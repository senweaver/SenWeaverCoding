// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod clipboard;
pub mod events;
pub mod url;
pub mod window;

use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::computer::recorder::{RecorderEvent, RecorderStatus};
use events::ActivityHub;

const WINDOW_POLL_MS: u64 = 1000;
const WINDOW_POLL_BROWSER_MS: u64 = 1600;
const URL_MIN_INTERVAL_MS: u64 = 1500;
const CLIPBOARD_POLL_MS: u64 = 700;
const COUNT_NOTIFY_EVERY: u64 = 25;

pub struct ActivityCapture {
    pub hub: Arc<ActivityHub>,
    cancel: CancellationToken,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl ActivityCapture {
    pub fn start(
        dir: &Path,
        event_tx: UnboundedSender<RecorderEvent>,
        task: &str,
    ) -> Self {
        let hub = ActivityHub::start(dir);
        hub.publish(
            events::EVENT_SESSION_START,
            "recorder",
            serde_json::json!({
                "platform": std::env::consts::OS,
                "task": task,
            }),
        );
        let cancel = CancellationToken::new();
        let mut tasks = Vec::new();
        if cfg!(windows) {
            tasks.push(tokio::spawn(window_collector(
                Arc::clone(&hub),
                cancel.clone(),
                event_tx,
            )));
            tasks.push(tokio::spawn(clipboard_collector(
                Arc::clone(&hub),
                cancel.clone(),
            )));
        }
        Self { hub, cancel, tasks }
    }

    pub fn marker(&self, note: &str) {
        self.hub.publish(
            events::EVENT_MARKER,
            "user",
            serde_json::json!({ "note": note }),
        );
    }

    pub async fn stop(self) {
        self.cancel.cancel();
        for task in self.tasks {
            let _ = task.await;
        }
        self.hub
            .publish(events::EVENT_SESSION_STOP, "recorder", serde_json::json!({}));
        self.hub.flush().await;
    }
}

async fn window_collector(
    hub: Arc<ActivityHub>,
    cancel: CancellationToken,
    event_tx: UnboundedSender<RecorderEvent>,
) {
    let mut last_app: Option<String> = None;
    let mut last_title: Option<String> = None;
    let mut last_url: Option<String> = None;
    let mut last_url_read = tokio::time::Instant::now() - std::time::Duration::from_secs(60);
    let mut browser_foreground = false;
    let mut last_notified_count = 0u64;

    loop {
        let poll_ms = if browser_foreground {
            WINDOW_POLL_BROWSER_MS
        } else {
            WINDOW_POLL_MS
        };
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_millis(poll_ms)) => {}
        }

        let Some(info) = tokio::task::spawn_blocking(window::read_foreground_window)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };

        browser_foreground = window::is_browser_app(&info.stem);
        let app_changed = last_app.as_deref() != Some(info.app.as_str());
        let title_changed = last_title.as_deref() != Some(info.title.as_str());

        if app_changed {
            hub.publish(
                events::EVENT_APP_ACTIVATE,
                "window",
                serde_json::json!({
                    "app": info.app,
                    "title": info.title,
                    "pid": info.pid,
                    "path": info.path,
                }),
            );
            last_app = Some(info.app.clone());
            last_title = Some(info.title.clone());
        } else if title_changed && !info.title.is_empty() {
            hub.publish(
                events::EVENT_APP_TITLE_CHANGE,
                "window",
                serde_json::json!({
                    "app": info.app,
                    "title": info.title,
                }),
            );
            last_title = Some(info.title.clone());
        }

        if browser_foreground {
            let elapsed_ms = last_url_read.elapsed().as_millis() as u64;
            if app_changed || elapsed_ms >= URL_MIN_INTERVAL_MS {
                last_url_read = tokio::time::Instant::now();
                if let Some(url_value) = url::read_browser_url().await {
                    let url_changed = last_url.as_deref() != Some(url_value.as_str());
                    if url_changed || app_changed {
                        hub.publish(
                            events::EVENT_BROWSER_URL,
                            "browser",
                            serde_json::json!({
                                "app": info.app,
                                "url": url_value,
                                "host": url::host_of(&url_value),
                                "title": info.title,
                            }),
                        );
                        last_url = Some(url_value);
                    }
                }
            }
        }

        let count = hub.event_count();
        if count >= last_notified_count + COUNT_NOTIFY_EVERY {
            last_notified_count = count;
            let _ = event_tx.send(RecorderEvent::status_code(
                RecorderStatus::Recording,
                "recorder_activity_count",
                format!("{count}"),
            ));
        }
    }
}

async fn clipboard_collector(hub: Arc<ActivityHub>, cancel: CancellationToken) {
    let mut last_seq = clipboard::clipboard_sequence_number();
    let mut last_hash: Option<String> = None;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_millis(CLIPBOARD_POLL_MS)) => {}
        }
        let seq = clipboard::clipboard_sequence_number();
        if seq == last_seq {
            continue;
        }
        last_seq = seq;
        let Some(snapshot) = tokio::task::spawn_blocking(clipboard::read_clipboard_snapshot)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        if last_hash.as_deref() == Some(snapshot.hash.as_str()) {
            continue;
        }
        last_hash = Some(snapshot.hash.clone());
        hub.publish(
            events::EVENT_CLIPBOARD_CHANGE,
            "clipboard",
            serde_json::json!({
                "formats": snapshot.formats,
                "length": snapshot.length,
                "hash": snapshot.hash,
                "textPreview": snapshot.text_preview,
            }),
        );
    }
}
