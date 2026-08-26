// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};

pub const EVENT_SESSION_START: &str = "session.start";
pub const EVENT_SESSION_STOP: &str = "session.stop";
pub const EVENT_MARKER: &str = "marker";
pub const EVENT_APP_ACTIVATE: &str = "app.activate";
pub const EVENT_APP_TITLE_CHANGE: &str = "app.title-change";
pub const EVENT_CLIPBOARD_CHANGE: &str = "clipboard.change";
pub const EVENT_BROWSER_URL: &str = "browser.url";

pub const MEANINGFUL_EVENT_TYPES: [&str; 12] = [
    EVENT_APP_ACTIVATE,
    EVENT_APP_TITLE_CHANGE,
    EVENT_BROWSER_URL,
    EVENT_CLIPBOARD_CHANGE,
    EVENT_MARKER,
    "input.click",
    "input.double_click",
    "input.right_click",
    "input.drag",
    "input.scroll",
    "input.type",
    "input.key_press",
];

pub fn is_meaningful_event(kind: &str) -> bool {
    MEANINGFUL_EVENT_TYPES.contains(&kind)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub seq: u64,
    pub t: u64,
    pub epoch: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub source: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

enum WriterMessage {
    Event(ActivityEvent),
    Flush(oneshot::Sender<()>),
}

pub struct ActivityHub {
    tx: mpsc::UnboundedSender<WriterMessage>,
    seq: AtomicU64,
    count: AtomicU64,
    started_epoch_ms: i64,
    started: std::time::Instant,
}

impl ActivityHub {
    pub fn start(dir: &Path) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let path = dir.join("events.jsonl");
        tokio::spawn(writer_loop(path, rx));
        Arc::new(Self {
            tx,
            seq: AtomicU64::new(0),
            count: AtomicU64::new(0),
            started_epoch_ms: chrono::Utc::now().timestamp_millis(),
            started: std::time::Instant::now(),
        })
    }

    pub fn publish(&self, kind: &str, source: &str, payload: serde_json::Value) {
        let event = ActivityEvent {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            t: self.started.elapsed().as_millis() as u64,
            epoch: chrono::Utc::now().timestamp_millis(),
            kind: kind.to_string(),
            source: source.to_string(),
            payload,
        };
        self.count.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(WriterMessage::Event(event));
    }

    pub fn event_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn started_epoch_ms(&self) -> i64 {
        self.started_epoch_ms
    }

    pub async fn flush(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self.tx.send(WriterMessage::Flush(ack_tx)).is_ok() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), ack_rx).await;
        }
    }
}

async fn writer_loop(path: PathBuf, mut rx: mpsc::UnboundedReceiver<WriterMessage>) {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await;
    let mut file = match file {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("activity event log open failed for {}: {e}", path.display());
            while let Some(msg) = rx.recv().await {
                if let WriterMessage::Flush(ack) = msg {
                    let _ = ack.send(());
                }
            }
            return;
        }
    };
    while let Some(msg) = rx.recv().await {
        match msg {
            WriterMessage::Event(event) => {
                let mut line = match serde_json::to_string(&event) {
                    Ok(line) => line,
                    Err(_) => continue,
                };
                line.push('\n');
                if file.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
            WriterMessage::Flush(ack) => {
                let _ = file.flush().await;
                let _ = ack.send(());
            }
        }
    }
    let _ = file.flush().await;
}

pub fn read_events(dir: &Path) -> Vec<ActivityEvent> {
    let path = dir.join("events.jsonl");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<ActivityEvent>(line).ok())
        .collect()
}
