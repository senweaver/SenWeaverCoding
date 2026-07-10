// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::observability::session_write_mode_metrics;
use crate::session::event::SessionEvent;
use crate::session::state::{SessionId, SessionState};

pub const SNAPSHOT_EVERY: u64 = 100;

const SNAPSHOT_MIN_INTERVAL_MS: u64 = 5_000;

const EVENTS_FILE: &str = "events.jsonl";
const SNAPSHOT_FILE: &str = "snapshot.json";
const ABSORBED_FILE: &str = "events.absorbed";
const WRITE_LOCK_FILE: &str = "session.write.lock";

const WRITE_QUEUE_CAPACITY: usize = 16384;

const LOCK_TOUCH_INTERVAL_SECS: u64 = 30;

const WRITER_FLUSH_MAX_ATTEMPTS: u32 = 3;

const WRITER_RETRY_BACKOFF_MS: u64 = 25;

enum SessionLogMsg {
    Append(SessionEvent),
    Snapshot(Box<SessionState>),
}

pub struct SessionEventLog {
    root: PathBuf,
    id: SessionId,
    writer: Mutex<Option<mpsc::SyncSender<SessionLogMsg>>>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,

    read_only: bool,

    since_snapshot: AtomicU64,

    last_snapshot_at_ms: AtomicU64,

    seq: AtomicU64,
    snapshot_every: u64,

    write_degraded: Arc<AtomicBool>,

    write_failures: Arc<AtomicU64>,
}

fn monotonic_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(std::time::Instant::now);
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub fn latest_session_id(workspace_root: &Path) -> Option<String> {
    let sessions_root = workspace_root.join(".sen").join("sessions");
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for entry in std::fs::read_dir(&sessions_root).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let mut newest: Option<std::time::SystemTime> = None;
        let mut consider = |p: PathBuf| {
            if let Ok(modified) = std::fs::metadata(&p).and_then(|m| m.modified()) {
                if newest.is_none_or(|current| modified > current) {
                    newest = Some(modified);
                }
            }
        };
        consider(path.join(EVENTS_FILE));
        consider(path.join(SNAPSHOT_FILE));
        for rotated in rotated_event_logs(&path) {
            consider(rotated);
        }
        let Some(modified) = newest else { continue };
        if best
            .as_ref()
            .is_none_or(|(current, _)| modified > *current)
        {
            best = Some((modified, id));
        }
    }
    best.map(|(_, id)| id)
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

        let existing = std::fs::metadata(&events_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let mut start_seq = 0_u64;
        if existing > 0 {
            let mut reader = BufReader::with_capacity(256 * 1024, File::open(&events_path)?);
            let mut count: u64 = 0;
            let mut last_byte: u8 = b'\n';
            loop {
                let buf = reader.fill_buf()?;
                if buf.is_empty() {
                    break;
                }
                count += buf.iter().filter(|&&b| b == b'\n').count() as u64;
                last_byte = buf[buf.len() - 1];
                let consumed = buf.len();
                reader.consume(consumed);
            }
            if last_byte != b'\n' {
                count += 1;
            }
            start_seq = count;
        }

        let lock_path = dir.join(WRITE_LOCK_FILE);
        let lock = match crate::session::write_lock::SessionWriteLock::acquire(&lock_path) {
            Ok(lock) => lock,
            Err(err) => {
                tracing::warn!(
                    session_id = %id,
                    lock = %lock_path.display(),
                    error = %err,
                    "failed to probe session write lock; entering read-only mode"
                );
                None
            }
        };

        let write_degraded = Arc::new(AtomicBool::new(false));
        let write_failures = Arc::new(AtomicU64::new(0));

        let (writer, handle, read_only) = match lock {
            Some(lock) => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&events_path)?;
                let (tx, rx) = mpsc::sync_channel::<SessionLogMsg>(WRITE_QUEUE_CAPACITY);
                let writer_root = dir.clone();
                let loop_degraded = Arc::clone(&write_degraded);
                let loop_failures = Arc::clone(&write_failures);
                let handle = std::thread::Builder::new()
                    .name("session-event-log".to_string())
                    .spawn(move || {
                        session_writer_loop(
                            writer_root,
                            file,
                            rx,
                            lock,
                            loop_degraded,
                            loop_failures,
                        )
                    })?;
                (Some(tx), Some(handle), false)
            }
            None => {
                tracing::error!(
                    session_id = %id,
                    lock = %lock_path.display(),
                    "another process holds the session write lock; session persistence disabled (read-only mode)"
                );
                (None, None, true)
            }
        };

        Ok(Self {
            root: dir,
            id: id.to_string(),
            writer: Mutex::new(writer),
            handle: Mutex::new(handle),
            read_only,
            since_snapshot: AtomicU64::new(0),
            last_snapshot_at_ms: AtomicU64::new(0),
            seq: AtomicU64::new(start_seq),
            snapshot_every: SNAPSHOT_EVERY,
            write_degraded,
            write_failures,
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

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn is_degraded(&self) -> bool {
        self.write_degraded.load(Ordering::Relaxed)
    }

    pub fn write_failures(&self) -> u64 {
        self.write_failures.load(Ordering::Relaxed)
    }

    pub fn append(&self, evt: &SessionEvent) -> std::io::Result<u64> {
        if self.read_only {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "session write lock held by another process; event log is read-only",
            ));
        }
        let guard = self.writer.lock().unwrap_or_else(|poison| poison.into_inner());
        let tx = guard.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "session event log writer is closed",
            )
        })?;
        match tx.try_send(SessionLogMsg::Append(evt.clone())) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "session event log write queue is full (writer falling behind)",
                ));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "session event log writer thread is gone",
                ));
            }
        }
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let _bumped = self.since_snapshot.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(seq)
    }

    pub fn needs_snapshot(&self) -> bool {
        if self.since_snapshot.load(Ordering::Relaxed) < self.snapshot_every {
            return false;
        }
        let last = self.last_snapshot_at_ms.load(Ordering::Relaxed);
        last == 0 || monotonic_ms().saturating_sub(last) >= SNAPSHOT_MIN_INTERVAL_MS
    }

    pub fn write_snapshot(&self, state: &SessionState) -> std::io::Result<()> {
        if self.read_only {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "session write lock held by another process; snapshot skipped",
            ));
        }
        let guard = self.writer.lock().unwrap_or_else(|poison| poison.into_inner());
        let tx = guard.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "session event log writer is closed",
            )
        })?;
        match tx.try_send(SessionLogMsg::Snapshot(Box::new(state.clone()))) {
            Ok(()) => {
                self.since_snapshot.store(0, Ordering::SeqCst);
                self.last_snapshot_at_ms
                    .store(monotonic_ms().max(1), Ordering::SeqCst);
                Ok(())
            }
            Err(mpsc::TrySendError::Full(_)) => Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "session event log write queue is full; snapshot deferred",
            )),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "session event log writer thread is gone",
            )),
        }
    }

    pub fn replay(&self, id: &str) -> std::io::Result<Option<SessionState>> {
        let snap_path = self.root.join(SNAPSHOT_FILE);
        let events_path = self.root.join(EVENTS_FILE);

        let mut snapshot_corrupt = false;
        let mut state: Option<SessionState> = None;

        if snap_path.exists() {
            match std::fs::read(&snap_path) {
                Ok(raw) => match serde_json::from_slice::<SessionState>(&raw) {
                    Ok(parsed) => state = Some(parsed),
                    Err(err) => {
                        snapshot_corrupt = true;
                        let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
                        let backup = self.root.join(format!("{SNAPSHOT_FILE}.corrupt.{ts}"));
                        match std::fs::rename(&snap_path, &backup) {
                            Ok(()) => tracing::error!(
                                session_id = %id,
                                error = %err,
                                backup = %backup.display(),
                                "session snapshot is corrupt; backed it up and rebuilding state from event logs"
                            ),
                            Err(rename_err) => tracing::error!(
                                session_id = %id,
                                error = %err,
                                rename_error = %rename_err,
                                "session snapshot is corrupt and could not be backed up; rebuilding state from event logs"
                            ),
                        }
                    }
                },
                Err(err) => {
                    snapshot_corrupt = true;
                    tracing::error!(
                        session_id = %id,
                        error = %err,
                        "session snapshot could not be read; rebuilding state from event logs"
                    );
                }
            }
        }

        let mut recovered_any = state.is_some();
        let mut working = state.take().unwrap_or_else(|| SessionState::new(id));

        if snapshot_corrupt {
            for rotated in rotated_event_logs(&self.root) {
                match apply_event_file(&rotated, id, &mut working) {
                    Ok(applied) => recovered_any = recovered_any || applied,
                    Err(err) => tracing::warn!(
                        session_id = %id,
                        file = %rotated.display(),
                        error = %err,
                        "failed to replay rotated session event log during recovery"
                    ),
                }
            }
        }

        if events_path.exists() {
            // Skip the leading active-log lines the snapshot already absorbed
            // (set when a crash happened between snapshot write and log rotation);
            // 0 in the normal case. When the snapshot was corrupt we rebuilt from
            // rotated logs above, so the whole active log must be replayed.
            let skip = if snapshot_corrupt {
                0
            } else {
                read_absorbed_marker(&self.root)
            };
            apply_event_file_skipping(&events_path, id, &mut working, skip)?;
            recovered_any = true;
        }

        Ok(if recovered_any { Some(working) } else { None })
    }

}

fn apply_event_file(
    path: &Path,
    id: &str,
    state: &mut SessionState,
) -> std::io::Result<bool> {
    apply_event_file_skipping(path, id, state, 0)
}

fn apply_event_file_skipping(
    path: &Path,
    id: &str,
    state: &mut SessionState,
    skip_nonempty: u64,
) -> std::io::Result<bool> {
    let reader = BufReader::new(File::open(path)?);
    let mut applied = false;
    let mut skipped = 0u64;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if skipped < skip_nonempty {
            skipped += 1;
            continue;
        }
        match serde_json::from_str::<SessionEvent>(&line) {
            Ok(evt) => {
                state.apply(&evt);
                applied = true;
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %id,
                    file = %path.display(),
                    error = %err,
                    "skipping malformed event log line"
                );
            }
        }
    }
    Ok(applied)
}

fn rotated_event_logs(root: &Path) -> Vec<PathBuf> {
    let mut rotated: Vec<(u64, PathBuf)> = match std::fs::read_dir(root) {
        Ok(iter) => iter
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?.to_string();
                if name == EVENTS_FILE {
                    return None;
                }
                let version = name
                    .strip_prefix("events.")?
                    .strip_suffix(".jsonl")?
                    .parse::<u64>()
                    .ok()?;
                Some((version, path))
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    rotated.sort_by_key(|(version, _)| *version);
    rotated.into_iter().map(|(_, path)| path).collect()
}

impl Drop for SessionEventLog {
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

fn session_writer_loop(
    root: PathBuf,
    file: File,
    rx: mpsc::Receiver<SessionLogMsg>,
    lock: crate::session::write_lock::SessionWriteLock,
    write_degraded: Arc<AtomicBool>,
    write_failures: Arc<AtomicU64>,
) {
    let mut writer = BufWriter::new(file);
    loop {
        let first = match rx.recv_timeout(Duration::from_secs(LOCK_TOUCH_INTERVAL_SECS)) {
            Ok(msg) => msg,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock.touch();
                if lock.is_degraded() {
                    write_degraded.store(true, Ordering::Relaxed);
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let mut dirty = false;
        let mut batch_failed = false;
        let mut next = Some(first);
        while let Some(msg) = next {
            match msg {
                SessionLogMsg::Append(evt) => match serde_json::to_string(&evt) {
                    Ok(line) => {
                        let result = writer
                            .write_all(line.as_bytes())
                            .and_then(|()| writer.write_all(b"\n"));
                        match result {
                            Ok(()) => dirty = true,
                            Err(e) => {
                                batch_failed = true;
                                let failures =
                                    write_failures.fetch_add(1, Ordering::Relaxed) + 1;
                                write_degraded.store(true, Ordering::Relaxed);
                                tracing::error!(
                                    error = %e,
                                    total_failures = failures,
                                    "session event log append failed; persistence degraded (event not durably buffered)"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "session event serialize failed");
                    }
                },
                SessionLogMsg::Snapshot(state) => {
                    match write_snapshot_to_disk(&root, &mut writer, &state) {
                        Ok(()) => {
                            dirty = false;
                            session_write_mode_metrics::incr_session_snapshot_written();
                        }
                        Err(e) => {
                            batch_failed = true;
                            let failures = write_failures.fetch_add(1, Ordering::Relaxed) + 1;
                            write_degraded.store(true, Ordering::Relaxed);
                            tracing::error!(
                                error = %e,
                                total_failures = failures,
                                "session snapshot write failed; persistence degraded"
                            );
                        }
                    }
                }
            }
            next = rx.try_recv().ok();
        }
        if dirty {
            if flush_with_retry(&mut writer, &write_failures) {
                if !batch_failed {
                    write_degraded.store(false, Ordering::Relaxed);
                }
            } else {
                write_degraded.store(true, Ordering::Relaxed);
            }
        } else if !batch_failed {
            write_degraded.store(false, Ordering::Relaxed);
        }
        lock.touch();
        if lock.is_degraded() {
            write_degraded.store(true, Ordering::Relaxed);
        }
    }
    let _ = writer.flush();
    drop(lock);
}

fn flush_with_retry(writer: &mut BufWriter<File>, write_failures: &AtomicU64) -> bool {
    let mut attempt = 0;
    loop {
        match writer.flush() {
            Ok(()) => return true,
            Err(e) => {
                attempt += 1;
                if attempt >= WRITER_FLUSH_MAX_ATTEMPTS {
                    let failures = write_failures.fetch_add(1, Ordering::Relaxed) + 1;
                    tracing::error!(
                        error = %e,
                        total_failures = failures,
                        attempts = attempt,
                        "session event log flush failed after retries; persistence degraded (buffered events may be lost)"
                    );
                    return false;
                }
                std::thread::sleep(Duration::from_millis(
                    WRITER_RETRY_BACKOFF_MS * u64::from(attempt),
                ));
            }
        }
    }
}

fn count_nonempty_lines(path: &Path) -> u64 {
    let Ok(file) = File::open(path) else {
        return 0;
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .count() as u64
}

fn write_absorbed_marker(root: &Path, count: u64) {
    let path = root.join(ABSORBED_FILE);
    if count == 0 {
        let _ = std::fs::remove_file(&path);
        return;
    }
    let _ = crate::util::atomic_write(&path, count.to_string().as_bytes());
}

fn read_absorbed_marker(root: &Path) -> u64 {
    std::fs::read_to_string(root.join(ABSORBED_FILE))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn write_snapshot_to_disk(
    root: &Path,
    writer: &mut BufWriter<File>,
    state: &SessionState,
) -> std::io::Result<()> {
    let active = root.join(EVENTS_FILE);
    // Flush pending events first so the on-disk active log matches what the
    // snapshot is about to absorb.
    writer.flush()?;
    // Record how many active-log lines this snapshot already reflects. If we
    // crash after writing the snapshot but before rotating the active log, replay
    // uses this marker to skip the already-absorbed prefix and avoid re-applying
    // (which would duplicate turns/tool calls, since apply() is append-style).
    let absorbed = count_nonempty_lines(&active);

    let snap_path = root.join(SNAPSHOT_FILE);
    let tmp_path = root.join(format!("{SNAPSHOT_FILE}.tmp"));
    let json = serde_json::to_vec(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    {
        use std::io::Write as _;
        let mut tmp = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)?;
        tmp.write_all(&json)?;
        // fsync before rename so a crash cannot leave an empty/half-written
        // snapshot that would discard most of the session history on replay.
        tmp.sync_all()?;
    }
    std::fs::rename(&tmp_path, &snap_path)?;
    // Snapshot is now durable and reflects `absorbed` leading active-log lines.
    write_absorbed_marker(root, absorbed);

    let rotated = root.join(format!("events.{}.jsonl", state.version));
    let mut rotation_failed = false;
    if active.exists() {
        if let Err(e) = std::fs::rename(&active, &rotated) {
            rotation_failed = true;
            tracing::warn!(
                error = %e,
                active = %active.display(),
                "failed to rotate session event log after snapshot; truncating active log because its events are already durable in the snapshot"
            );
        }
    }
    let new_file = if rotation_failed {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&active)?
    } else {
        OpenOptions::new()
            .create(true)
            .append(true)
            .truncate(false)
            .open(&active)?
    };
    *writer = BufWriter::new(new_file);
    // Active log is now fresh (rotated away or truncated), so nothing in it is
    // already absorbed by the snapshot.
    write_absorbed_marker(root, 0);
    prune_rotated(root, 3);
    Ok(())
}

fn prune_rotated(root: &Path, keep: usize) {
    let rotated = rotated_event_logs(root);
    if rotated.len() <= keep {
        return;
    }
    let drop_count = rotated.len() - keep;
    for path in rotated.into_iter().take(drop_count) {
        let _ = std::fs::remove_file(path);
    }
}
