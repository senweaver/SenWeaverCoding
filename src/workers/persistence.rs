// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use crate::session::event::SessionEvent;
use crate::workers::events::{WorkerMeta, WorkerStatus};

const EVENTS_FILE: &str = "events.jsonl";
const META_FILE: &str = "meta.json";

const EVENT_CHANNEL_CAPACITY: usize = 8192;

pub struct WorkerEventLog {
    root: PathBuf,
    writer: Mutex<Option<mpsc::SyncSender<SessionEvent>>>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    seq: AtomicU64,
}

impl WorkerEventLog {
    pub fn open<P: AsRef<Path>>(workspace_root: P, worker_id: &str) -> std::io::Result<Self> {
        let dir = worker_dir(workspace_root.as_ref(), worker_id);
        std::fs::create_dir_all(&dir)?;

        let events_path = dir.join(EVENTS_FILE);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)?;

        let existing = std::fs::metadata(&events_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let mut start_seq = 0_u64;
        if existing > 0 {
            let reader = BufReader::new(File::open(&events_path)?);
            start_seq = reader.lines().map_while(Result::ok).count() as u64;
        }

        let (tx, rx) = mpsc::sync_channel::<SessionEvent>(EVENT_CHANNEL_CAPACITY);
        let handle = std::thread::Builder::new()
            .name("worker-event-log".to_string())
            .spawn(move || worker_writer_loop(file, rx))
            .ok();

        Ok(Self {
            root: dir,
            writer: Mutex::new(Some(tx)),
            handle: Mutex::new(handle),
            seq: AtomicU64::new(start_seq),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn append(&self, evt: &SessionEvent) -> std::io::Result<u64> {
        let mut guard = self
            .writer
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let needs_rebuild = match guard.as_ref() {
            Some(tx) => match tx.try_send(evt.clone()) {
                Ok(()) => false,
                Err(mpsc::TrySendError::Full(pending)) => tx.send(pending).is_err(),
                Err(mpsc::TrySendError::Disconnected(_)) => true,
            },
            None => true,
        };
        if needs_rebuild {
            tracing::warn!("worker event log writer thread is gone; rebuilding writer");
            match self.rebuild_writer() {
                Ok((tx, handle)) => {
                    if tx.send(evt.clone()).is_err() {
                        tracing::error!(
                            "worker event log writer rebuild produced a dead channel; append dropped"
                        );
                        *guard = None;
                    } else {
                        *guard = Some(tx);
                        let mut handle_guard = self
                            .handle
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner());
                        if let Some(old) = handle_guard.take() {
                            let _ = old.join();
                        }
                        *handle_guard = Some(handle);
                    }
                }
                Err(err) => {
                    tracing::error!(
                        error = %err,
                        "worker event log writer rebuild failed; append dropped"
                    );
                    *guard = None;
                }
            }
        }
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(seq)
    }

    fn rebuild_writer(
        &self,
    ) -> std::io::Result<(mpsc::SyncSender<SessionEvent>, std::thread::JoinHandle<()>)> {
        let events_path = self.root.join(EVENTS_FILE);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)?;
        let (tx, rx) = mpsc::sync_channel::<SessionEvent>(EVENT_CHANNEL_CAPACITY);
        let handle = std::thread::Builder::new()
            .name("worker-event-log".to_string())
            .spawn(move || worker_writer_loop(file, rx))?;
        Ok((tx, handle))
    }

    pub fn replay(&self) -> std::io::Result<Vec<SessionEvent>> {
        let path = self.root.join(EVENTS_FILE);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let reader = BufReader::new(File::open(&path)?);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<SessionEvent>(&line) {
                Ok(evt) => events.push(evt),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        path = %path.display(),
                        "skipping malformed worker event line"
                    );
                }
            }
        }
        Ok(events)
    }
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

fn worker_writer_loop(file: File, rx: mpsc::Receiver<SessionEvent>) {
    fn write_event(writer: &mut BufWriter<File>, evt: &SessionEvent) {
        match serde_json::to_string(evt) {
            Ok(line) => {
                let result = writer
                    .write_all(line.as_bytes())
                    .and_then(|()| writer.write_all(b"\n"));
                if let Err(e) = result {
                    tracing::warn!(error = %e, "worker event log append failed");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "worker event serialize failed");
            }
        }
    }

    let mut writer = BufWriter::new(file);
    while let Ok(evt) = rx.recv() {
        write_event(&mut writer, &evt);
        while let Ok(more) = rx.try_recv() {
            write_event(&mut writer, &more);
        }
        if let Err(e) = writer.flush() {
            tracing::warn!(error = %e, "worker event log flush failed");
        }
    }
    let _ = writer.flush();
}

pub fn workers_root<P: AsRef<Path>>(workspace_root: P) -> PathBuf {
    workspace_root.as_ref().join(".sen").join("workers")
}

pub fn worker_dir<P: AsRef<Path>>(workspace_root: P, worker_id: &str) -> PathBuf {
    workers_root(workspace_root).join(worker_id)
}

pub fn write_meta<P: AsRef<Path>>(
    workspace_root: P,
    meta: &WorkerMeta,
) -> std::io::Result<()> {
    let dir = worker_dir(workspace_root, &meta.worker_id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(META_FILE);
    let tmp = dir.join(format!(
        "{META_FILE}.tmp.{}",
        uuid::Uuid::new_v4().simple()
    ));
    let bytes = serde_json::to_vec_pretty(meta).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;
    std::fs::write(&tmp, &bytes)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
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
    let meta: WorkerMeta = serde_json::from_slice(&raw).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;
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
    let metas = list_meta(workspace_root)?;
    Ok(metas.into_iter().filter(|m| !m.status.is_terminal()).collect())
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
