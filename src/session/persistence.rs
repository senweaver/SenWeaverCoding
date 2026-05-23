// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::observability::session_write_mode_metrics;
use crate::session::event::SessionEvent;
use crate::session::state::{SessionId, SessionState};

pub const SNAPSHOT_EVERY: u64 = 100;

const EVENTS_FILE: &str = "events.jsonl";
const SNAPSHOT_FILE: &str = "snapshot.json";

pub struct SessionEventLog {
    root: PathBuf,
    id: SessionId,
    writer: Mutex<BufWriter<File>>,

    since_snapshot: AtomicU64,

    seq: AtomicU64,
    snapshot_every: u64,
}

impl SessionEventLog {

    pub fn open_at<P: AsRef<Path>>(root: P, id: &str) -> std::io::Result<Self> {
        let dir = root
            .as_ref()
            .join(".sen")
            .join("sessions")
            .join(id);
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

        Ok(Self {
            root: dir,
            id: id.to_string(),
            writer: Mutex::new(BufWriter::new(file)),
            since_snapshot: AtomicU64::new(0),
            seq: AtomicU64::new(start_seq),
            snapshot_every: SNAPSHOT_EVERY,
        })
    }

    #[must_use]
    pub fn with_snapshot_every(mut self, n: u64) -> Self {
        self.snapshot_every = n.max(1);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn session_id(&self) -> &str {
        &self.id
    }

    pub fn append(&self, evt: &SessionEvent) -> std::io::Result<u64> {
        let line = serde_json::to_string(evt).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        {
            let mut w = self
                .writer
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            w.write_all(line.as_bytes())?;
            w.write_all(b"\n")?;
            w.flush()?;
        }
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let _bumped = self.since_snapshot.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(seq)
    }

    pub fn needs_snapshot(&self) -> bool {
        self.since_snapshot.load(Ordering::Relaxed) >= self.snapshot_every
    }

    pub fn write_snapshot(&self, state: &SessionState) -> std::io::Result<()> {
        let snap_path = self.root.join(SNAPSHOT_FILE);
        let tmp_path = self.root.join(format!("{SNAPSHOT_FILE}.tmp"));
        let json = serde_json::to_vec_pretty(state).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &snap_path)?;

        let rotated = self
            .root
            .join(format!("events.{}.jsonl", state.version));
        let active = self.root.join(EVENTS_FILE);
        {
            let mut w = self
                .writer
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            w.flush()?;
        }
        if active.exists() {
            let _ = std::fs::rename(&active, &rotated);
        }
        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .truncate(false)
            .open(&active)?;
        {
            let mut w = self
                .writer
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            *w = BufWriter::new(new_file);
        }
        self.since_snapshot.store(0, Ordering::SeqCst);
        self.prune_rotated(3);

        session_write_mode_metrics::incr_session_snapshot_written();
        Ok(())
    }

    pub fn replay(&self, id: &str) -> std::io::Result<Option<SessionState>> {
        let snap_path = self.root.join(SNAPSHOT_FILE);
        let events_path = self.root.join(EVENTS_FILE);

        let mut state = if snap_path.exists() {
            let raw = std::fs::read(&snap_path)?;
            let parsed: SessionState = serde_json::from_slice(&raw).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
            })?;
            Some(parsed)
        } else {
            None
        };

        if events_path.exists() {
            let reader = BufReader::new(File::open(&events_path)?);
            let mut applied_from_log = false;
            let mut working = state.take().unwrap_or_else(|| SessionState::new(id));
            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let evt: SessionEvent = match serde_json::from_str(&line) {
                    Ok(evt) => evt,
                    Err(err) => {
                        tracing::warn!(
                            session_id = %id,
                            error = %err,
                            "skipping malformed event log line"
                        );
                        continue;
                    }
                };
                working.apply(&evt);
                applied_from_log = true;
            }
            if applied_from_log || working.version > 0 {
                state = Some(working);
            } else {
                state = Some(working);
            }
        }

        Ok(state)
    }

    fn prune_rotated(&self, keep: usize) {
        let mut rotated: Vec<PathBuf> = match std::fs::read_dir(&self.root) {
            Ok(iter) => iter
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with("events.") && n.ends_with(".jsonl") && n != EVENTS_FILE)
                        .unwrap_or(false)
                })
                .collect(),
            Err(_) => return,
        };
        rotated.sort();
        if rotated.len() <= keep {
            return;
        }
        let drop_count = rotated.len() - keep;
        for path in rotated.into_iter().take(drop_count) {
            let _ = std::fs::remove_file(path);
        }
    }
}
