// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::agent::TurnEvent;
use crate::session::event::SessionEvent;
use crate::workers::events::{WorkerMeta, WorkerStatus};
use crate::workers::worker::SequencedWorkerEvent;

const EVENTS_FILE: &str = "events.jsonl";
const META_FILE: &str = "meta.json";

const EVENT_CHANNEL_CAPACITY: usize = 8192;

pub struct WorkerEventLog {
    root: PathBuf,
    writer: Mutex<Option<mpsc::Sender<EventWriteRequest>>>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    feed: tokio::sync::broadcast::Sender<SequencedWorkerEvent>,
}

struct EventWriteRequest {
    event: SessionEvent,
    live_event: TurnEvent,
    ack: oneshot::Sender<std::io::Result<u64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerEventRecord {
    pub seq: u64,
    pub event: SessionEvent,
}

impl WorkerEventLog {
    pub fn open<P: AsRef<Path>>(
        workspace_root: P,
        worker_id: &str,
        feed: tokio::sync::broadcast::Sender<SequencedWorkerEvent>,
    ) -> std::io::Result<Self> {
        let dir = worker_dir(workspace_root.as_ref(), worker_id);
        std::fs::create_dir_all(&dir)?;

        let events_path = dir.join(EVENTS_FILE);
        let start_seq = replay_event_records_path(&events_path)?
            .last()
            .map(|record| record.seq)
            .unwrap_or(0);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)?;

        let (tx, rx) = mpsc::channel::<EventWriteRequest>(EVENT_CHANNEL_CAPACITY);
        let feed_for_writer = feed.clone();
        let handle = std::thread::Builder::new()
            .name("worker-event-log".to_string())
            .spawn(move || worker_writer_loop(file, rx, feed_for_writer, start_seq))?;

        Ok(Self {
            root: dir,
            writer: Mutex::new(Some(tx)),
            handle: Mutex::new(Some(handle)),
            feed,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn append(&self, event: SessionEvent, live_event: TurnEvent) -> std::io::Result<u64> {
        let tx = self
            .writer
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "worker event log writer is closed",
                )
            })?;
        let (ack, receive_ack) = oneshot::channel();
        tx.send(EventWriteRequest {
            event,
            live_event,
            ack,
        })
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "worker event log writer is unavailable",
            )
        })?;
        receive_ack.await.map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "worker event log writer stopped before acknowledging append",
            )
        })?
    }

    pub fn replay(&self) -> std::io::Result<Vec<WorkerEventRecord>> {
        replay_event_records_path(&self.root.join(EVENTS_FILE))
    }

    pub fn feed(&self) -> tokio::sync::broadcast::Sender<SequencedWorkerEvent> {
        self.feed.clone()
    }
}

pub fn replay_worker_events<P: AsRef<Path>>(
    workspace_root: P,
    worker_id: &str,
) -> std::io::Result<Vec<WorkerEventRecord>> {
    replay_event_records_path(&worker_dir(workspace_root, worker_id).join(EVENTS_FILE))
}

fn replay_event_records_path(path: &Path) -> std::io::Result<Vec<WorkerEventRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let reader = BufReader::new(File::open(path)?);
    let mut events = Vec::new();
    let mut highwater = 0_u64;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(mut record) = serde_json::from_str::<WorkerEventRecord>(&line) {
            if record.seq <= highwater {
                record.seq = highwater.saturating_add(1);
            }
            highwater = record.seq;
            events.push(record);
            continue;
        }
        highwater = highwater.saturating_add(1);
        match serde_json::from_str::<SessionEvent>(&line) {
            Ok(event) => events.push(WorkerEventRecord {
                seq: highwater,
                event,
            }),
            Err(err) => tracing::warn!(
                error = %err,
                path = %path.display(),
                "skipping malformed worker event line"
            ),
        }
    }
    Ok(events)
}

impl Drop for WorkerEventLog {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.writer.lock() {
            guard.take();
        }
        if let Ok(mut guard) = self.handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }
}

fn worker_writer_loop(
    mut file: File,
    mut rx: mpsc::Receiver<EventWriteRequest>,
    feed: tokio::sync::broadcast::Sender<SequencedWorkerEvent>,
    mut seq: u64,
) {
    while let Some(first) = rx.blocking_recv() {
        let mut requests = vec![first];
        while let Ok(request) = rx.try_recv() {
            requests.push(request);
        }
        let start_len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        let mut encoded = Vec::new();
        let mut sequenced = Vec::with_capacity(requests.len());
        let mut next_seq = seq;
        let result = requests
            .into_iter()
            .try_for_each(|request| {
                next_seq = next_seq.saturating_add(1);
                let record = WorkerEventRecord {
                    seq: next_seq,
                    event: request.event,
                };
                let line = serde_json::to_vec(&record).map_err(|err| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())
                })?;
                encoded.extend_from_slice(&line);
                encoded.push(b'\n');
                sequenced.push((next_seq, request.live_event, request.ack));
                Ok::<(), std::io::Error>(())
            })
            .and_then(|()| file.write_all(&encoded))
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_data());
        match result {
            Ok(()) => {
                seq = next_seq;
                for (event_seq, event, ack) in sequenced {
                    let _ = feed.send(SequencedWorkerEvent {
                        seq: event_seq,
                        event,
                    });
                    let _ = ack.send(Ok(event_seq));
                }
            }
            Err(err) => {
                let _ = file.set_len(start_len);
                tracing::warn!(error = %err, "worker event log durable append failed");
                let kind = err.kind();
                let message = err.to_string();
                for (_, _, ack) in sequenced {
                    let _ = ack.send(Err(std::io::Error::new(kind, message.clone())));
                }
                break;
            }
        }
    }
}

pub fn workers_root<P: AsRef<Path>>(workspace_root: P) -> PathBuf {
    workspace_root.as_ref().join(".sen").join("workers")
}

pub fn worker_dir<P: AsRef<Path>>(workspace_root: P, worker_id: &str) -> PathBuf {
    workers_root(workspace_root).join(worker_id)
}

pub fn write_meta<P: AsRef<Path>>(workspace_root: P, meta: &WorkerMeta) -> std::io::Result<()> {
    let dir = worker_dir(workspace_root, &meta.worker_id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(META_FILE);
    let bytes = serde_json::to_vec_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    crate::util::atomic_write(&path, &bytes)
}

pub fn read_meta<P: AsRef<Path>>(
    workspace_root: P,
    worker_id: &str,
) -> std::io::Result<Option<WorkerMeta>> {
    let path = worker_dir(workspace_root, worker_id).join(META_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&path)?;
    let meta: WorkerMeta = serde_json::from_slice(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(Some(meta))
}

pub fn list_meta<P: AsRef<Path>>(workspace_root: P) -> std::io::Result<Vec<WorkerMeta>> {
    let workspace_root = workspace_root.as_ref();
    let root = workers_root(workspace_root);
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&root)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let meta_path = path.join(META_FILE);
        if !meta_path.exists() {
            continue;
        }
        match read_meta(workspace_root, &id) {
            Ok(Some(m)) => out.push(m),
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    worker_id = %id,
                    error = %err,
                    "failed to read worker meta during list"
                );
            }
        }
    }
    out.sort_by_key(|m| m.started_at);
    Ok(out)
}

pub fn scan_interrupted<P: AsRef<Path>>(workspace_root: P) -> std::io::Result<Vec<WorkerMeta>> {
    let workspace_root = workspace_root.as_ref();
    let metas = list_meta(workspace_root)?;
    let mut interrupted = Vec::new();
    for mut meta in metas.into_iter().filter(|meta| !meta.status.is_terminal()) {
        let terminal = replay_worker_events(workspace_root, &meta.worker_id)?
            .into_iter()
            .rev()
            .find_map(|record| match record.event.kind {
                crate::session::event::SessionEventKind::WorkerCompleted {
                    success,
                    summary,
                    ..
                } => Some((
                    if success {
                        WorkerStatus::Completed
                    } else {
                        WorkerStatus::Failed
                    },
                    summary,
                    record.event.timestamp,
                )),
                crate::session::event::SessionEventKind::WorkerStopped { reason, .. } => {
                    Some((WorkerStatus::Stopped, reason, record.event.timestamp))
                }
                _ => None,
            });
        if let Some((status, summary, finished_at)) = terminal {
            meta.status = status;
            meta.finished_at = Some(finished_at);
            if status == WorkerStatus::Completed {
                meta.output = Some(summary);
                meta.error = None;
            } else {
                meta.error = Some(summary);
            }
            write_meta(workspace_root, &meta)?;
        } else {
            interrupted.push(meta);
        }
    }
    Ok(interrupted)
}

pub fn mark_worker_failed(
    workspace_root: &Path,
    meta: &mut WorkerMeta,
    reason: &str,
) -> std::io::Result<()> {
    meta.status = WorkerStatus::Failed;
    meta.error = Some(reason.to_string());
    meta.finished_at = Some(chrono::Utc::now());
    write_meta(workspace_root, meta)
}

pub fn scan_and_recover<P: AsRef<Path>>(workspace_root: P) -> std::io::Result<usize> {
    let root = workspace_root.as_ref().to_path_buf();
    let metas = scan_interrupted(&root)?;
    let mut recovered = 0_usize;
    for mut meta in metas {
        if let Err(err) = mark_worker_failed(
            &root,
            &mut meta,
            "Worker did not finish before the host process restarted; marked as failed.",
        ) {
            tracing::warn!(
                worker_id = %meta.worker_id,
                error = %err,
                "failed to persist recovered worker meta"
            );
        } else {
            recovered += 1;
        }
    }
    Ok(recovered)
}

pub fn find_worker_root(candidates: &[PathBuf], worker_id: &str) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|root| worker_dir(root, worker_id).join(META_FILE).exists())
        .cloned()
}
