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

#[derive(serde::Serialize, serde::Deserialize)]
struct SnapshotEnvelope {
    absorbed: u64,
    absorbed_hash: String,
    state: SessionState,
}

enum SnapshotFailure {
    Recoverable(std::io::Error),
    Fatal(std::io::Error),
}

impl SnapshotFailure {
    fn error(&self) -> &std::io::Error {
        match self {
            Self::Recoverable(e) | Self::Fatal(e) => e,
        }
    }
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
                self.write_degraded.store(true, Ordering::Relaxed);
                let failures = self.write_failures.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::warn!(
                    session_id = %self.id,
                    total_failures = failures,
                    "session event log write queue full; deferring event to the ordered replay buffer instead of blocking"
                );
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "session event log write queue full; event deferred to the ordered replay buffer",
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

        let mut envelope_marker: Option<(u64, Option<u64>)> = None;
        if snap_path.exists() {
            match std::fs::read(&snap_path) {
                Ok(raw) => match parse_snapshot_payload(&raw) {
                    Ok((parsed, marker)) => {
                        state = Some(parsed);
                        envelope_marker = marker;
                    }
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
            let (skip, expected_hash) = if snapshot_corrupt {
                (0, None)
            } else {
                let mut chosen: (u64, Option<u64>) = (0, None);
                let file_marker = read_absorbed_marker(&self.root);
                for candidate in [envelope_marker, Some(file_marker)].into_iter().flatten() {
                    let (skip, hash) = candidate;
                    if skip == 0 {
                        continue;
                    }
                    if absorbed_prefix_matches(&events_path, skip, hash).unwrap_or(false) {
                        chosen = (skip, hash);
                        break;
                    }
                }
                chosen
            };
            apply_event_file_skipping(&events_path, id, &mut working, skip, expected_hash)?;
            recovered_any = true;
        }

        Ok(if recovered_any { Some(working) } else { None })
    }

}

fn absorbed_prefix_matches(
    path: &Path,
    skip: u64,
    expected_hash: Option<u64>,
) -> std::io::Result<bool> {
    use std::hash::Hasher as _;
    if skip == 0 {
        return Ok(true);
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut seen = 0u64;
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        hash_line(&mut hasher, &line);
        seen += 1;
        if seen == skip {
            break;
        }
    }
    if seen < skip {
        return Ok(false);
    }
    match expected_hash {
        Some(expected) => Ok(hasher.finish() == expected),
        None => Ok(true),
    }
}

fn parse_snapshot_payload(
    raw: &[u8],
) -> Result<(SessionState, Option<(u64, Option<u64>)>), serde_json::Error> {
    match serde_json::from_slice::<SnapshotEnvelope>(raw) {
        Ok(envelope) => {
            let hash = u64::from_str_radix(&envelope.absorbed_hash, 16).ok();
            Ok((envelope.state, Some((envelope.absorbed, hash))))
        }
        Err(_) => serde_json::from_slice::<SessionState>(raw).map(|state| (state, None)),
    }
}

fn apply_event_file(
    path: &Path,
    id: &str,
    state: &mut SessionState,
) -> std::io::Result<bool> {
    apply_event_file_skipping(path, id, state, 0, None)
}

fn apply_event_file_skipping(
    path: &Path,
    id: &str,
    state: &mut SessionState,
    skip_nonempty: u64,
    expected_prefix_hash: Option<u64>,
) -> std::io::Result<bool> {
    use std::hash::Hasher as _;
    let mut effective_skip = skip_nonempty;
    if let (Some(expected), true) = (expected_prefix_hash, skip_nonempty > 0) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut seen = 0u64;
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            hash_line(&mut hasher, &line);
            seen += 1;
            if seen == skip_nonempty {
                break;
            }
        }
        if seen < skip_nonempty || hasher.finish() != expected {
            tracing::error!(
                session_id = %id,
                file = %path.display(),
                marker_lines = skip_nonempty,
                lines_present = seen,
                "absorbed marker does not match the active event log prefix; replaying the whole log (some entries may be duplicated instead of silently dropped)"
            );
            effective_skip = 0;
        }
    }
    let reader = BufReader::new(File::open(path)?);
    let mut applied = false;
    let mut skipped = 0u64;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if skipped < effective_skip {
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
        let mut batch_started = std::time::Instant::now();
        let mut batch_processed = 0u64;
        while let Some(msg) = next {
            batch_processed += 1;
            if batch_processed.is_multiple_of(1024)
                || batch_started.elapsed().as_secs() >= LOCK_TOUCH_INTERVAL_SECS / 2
            {
                let _ = writer.flush();
                lock.touch();
                if lock.is_degraded() {
                    write_degraded.store(true, Ordering::Relaxed);
                }
                batch_started = std::time::Instant::now();
            }
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
                        Err(failure) => {
                            batch_failed = true;
                            let failures = write_failures.fetch_add(1, Ordering::Relaxed) + 1;
                            write_degraded.store(true, Ordering::Relaxed);
                            tracing::error!(
                                error = %failure.error(),
                                total_failures = failures,
                                "session snapshot write failed; persistence degraded"
                            );
                            if matches!(failure, SnapshotFailure::Fatal(_)) {
                                tracing::error!(
                                    "session writer stopping after fatal snapshot rotation failure; \
                                     subsequent appends will be buffered by the session actor"
                                );
                                let _ = writer.flush();
                                drop(lock);
                                return;
                            }
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
    let _ = writer.flush().and_then(|()| writer.get_ref().sync_data());
    drop(lock);
}

fn flush_with_retry(writer: &mut BufWriter<File>, write_failures: &AtomicU64) -> bool {
    let mut attempt = 0;
    loop {
        match writer.flush().and_then(|()| writer.get_ref().sync_data()) {
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

fn hash_line(hasher: &mut std::collections::hash_map::DefaultHasher, line: &str) {
    use std::hash::Hash as _;
    line.hash(hasher);
}

fn count_and_hash_nonempty_lines(path: &Path) -> (u64, u64) {
    use std::hash::Hasher as _;
    let Ok(file) = File::open(path) else {
        return (0, 0);
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut count = 0u64;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        count += 1;
        hash_line(&mut hasher, &line);
    }
    (count, hasher.finish())
}

fn write_absorbed_marker(root: &Path, count: u64, prefix_hash: u64) {
    let path = root.join(ABSORBED_FILE);
    if count == 0 {
        let _ = std::fs::remove_file(&path);
        return;
    }
    let _ = crate::util::atomic_write(
        &path,
        format!("{count}:{prefix_hash:016x}").as_bytes(),
    );
}

fn read_absorbed_marker(root: &Path) -> (u64, Option<u64>) {
    let Some(raw) = std::fs::read_to_string(root.join(ABSORBED_FILE)).ok() else {
        return (0, None);
    };
    let raw = raw.trim();
    match raw.split_once(':') {
        Some((count, hash)) => {
            let count = count.parse::<u64>().unwrap_or(0);
            let hash = u64::from_str_radix(hash, 16).ok();
            (count, hash)
        }
        None => (raw.parse::<u64>().unwrap_or(0), None),
    }
}

fn write_snapshot_to_disk(
    root: &Path,
    writer: &mut BufWriter<File>,
    state: &SessionState,
) -> Result<(), SnapshotFailure> {
    let active = root.join(EVENTS_FILE);
    writer.flush().map_err(SnapshotFailure::Recoverable)?;
    let (absorbed, absorbed_hash) = count_and_hash_nonempty_lines(&active);

    let snap_path = root.join(SNAPSHOT_FILE);
    let tmp_path = root.join(format!("{SNAPSHOT_FILE}.tmp"));
    let envelope = SnapshotEnvelope {
        absorbed,
        absorbed_hash: format!("{absorbed_hash:016x}"),
        state: state.clone(),
    };
    let json = serde_json::to_vec(&envelope).map_err(|e| {
        SnapshotFailure::Recoverable(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        ))
    })?;
    {
        use std::io::Write as _;
        let mut tmp = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(SnapshotFailure::Recoverable)?;
        tmp.write_all(&json).map_err(SnapshotFailure::Recoverable)?;
        tmp.sync_all().map_err(SnapshotFailure::Recoverable)?;
    }
    std::fs::rename(&tmp_path, &snap_path).map_err(SnapshotFailure::Recoverable)?;
    write_absorbed_marker(root, absorbed, absorbed_hash);

    let rotated = root.join(format!("events.{}.jsonl", state.version));
    let mut rotation_failed = false;
    let mut rotation_succeeded = false;
    if active.exists() {
        match std::fs::rename(&active, &rotated) {
            Ok(()) => rotation_succeeded = true,
            Err(e) => {
                rotation_failed = true;
                tracing::warn!(
                    error = %e,
                    active = %active.display(),
                    "failed to rotate session event log after snapshot; truncating active log because its events are already durable in the snapshot"
                );
            }
        }
    }
    let open_truncated = || {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&active)
    };
    let open_appending = || {
        OpenOptions::new()
            .create(true)
            .append(true)
            .truncate(false)
            .open(&active)
    };
    let (new_file, marker_after_reopen) = if rotation_failed {
        (open_truncated().map_err(SnapshotFailure::Recoverable)?, 0)
    } else {
        match open_appending() {
            Ok(file) => (file, 0),
            Err(open_err) if rotation_succeeded => {
                match std::fs::rename(&rotated, &active) {
                    Ok(()) => {
                        tracing::warn!(
                            error = %open_err,
                            "failed to reopen fresh session event log; rolled the rotation back to keep appends replayable"
                        );
                        (
                            open_appending().map_err(SnapshotFailure::Fatal)?,
                            absorbed,
                        )
                    }
                    Err(rollback_err) => {
                        tracing::error!(
                            error = %open_err,
                            rollback_error = %rollback_err,
                            rotated = %rotated.display(),
                            "failed to reopen fresh session event log AND failed to roll back rotation; stopping the writer so appends fail loudly instead of landing in the rotated file"
                        );
                        return Err(SnapshotFailure::Fatal(open_err));
                    }
                }
            }
            Err(open_err) => return Err(SnapshotFailure::Recoverable(open_err)),
        }
    };
    *writer = BufWriter::new(new_file);
    if marker_after_reopen > 0 {
        write_absorbed_marker(root, marker_after_reopen, absorbed_hash);
    } else {
        write_absorbed_marker(root, 0, 0);
    }
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
