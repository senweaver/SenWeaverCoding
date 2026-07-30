// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::{broadcast, oneshot};

const EVENT_CHANNEL_CAP: usize = 512;
const WAIT_ANNOUNCE_DELAY: Duration = Duration::from_millis(50);
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_SNAPSHOTS_PER_SESSION: usize = 1024;
const CLAIM_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ResourceKind {
    FileWrite { path: PathBuf },
    Browser,
    Shell,
    WorkspaceExclusive,
}

impl ResourceKind {
    pub fn kind_str(&self) -> &'static str {
        match self {
            ResourceKind::FileWrite { .. } => "file",
            ResourceKind::Browser => "browser",
            ResourceKind::Shell => "shell",
            ResourceKind::WorkspaceExclusive => "workspace",
        }
    }

    pub fn target_str(&self) -> String {
        match self {
            ResourceKind::FileWrite { path } => path.to_string_lossy().to_string(),
            ResourceKind::Browser
            | ResourceKind::Shell
            | ResourceKind::WorkspaceExclusive => String::new(),
        }
    }

    fn sort_key(&self) -> (u8, String) {
        match self {
            ResourceKind::FileWrite { path } => (0, path.to_string_lossy().to_string()),
            ResourceKind::Browser => (1, String::new()),
            ResourceKind::Shell => (2, String::new()),
            ResourceKind::WorkspaceExclusive => (3, String::new()),
        }
    }
}

#[derive(Clone, Debug)]
struct Holder {
    session_id: String,
    title: String,
    ref_count: usize,
    confirmed: bool,
    claim_deadline: Option<Instant>,
}

struct Pending {
    session_id: String,
    title: String,
    waker: oneshot::Sender<()>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum ResourceEvent {
    WaitStarted {
        session_id: String,
        kind: String,
        target: String,
        holder_session_id: String,
        holder_title: String,
    },
    WaitResolved {
        session_id: String,
        kind: String,
        target: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum AcquireError {
    #[error("Timed out waiting for resource lock on `{kind}` ({target})")]
    Timeout { kind: &'static str, target: String },
    #[error("Resource manager shutting down")]
    Shutdown,
}

#[derive(Clone, Copy, Debug)]
struct SnapshotRecord {
    mtime: SystemTime,
    written: bool,
}

#[derive(Default)]
struct Inner {
    holders: HashMap<(String, ResourceKind), Holder>,
    waiters: HashMap<(String, ResourceKind), VecDeque<Pending>>,
    read_snapshots: HashMap<(String, String, PathBuf), SnapshotRecord>,
    snapshot_counts: HashMap<(String, String), usize>,
    snapshot_evicted_sessions: HashSet<(String, String)>,
    os_locks: HashMap<(String, ResourceKind), crate::session::os_lock::OsAdvisoryLock>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaiterSnapshot {
    pub kind: String,
    pub target: String,
    pub holder_session_id: String,
    pub holder_title: String,
}

pub struct WorkspaceResourceManager {
    inner: Mutex<Inner>,
    events: broadcast::Sender<ResourceEvent>,
}

impl WorkspaceResourceManager {
    pub fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAP);
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
            events,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ResourceEvent> {
        self.events.subscribe()
    }

    pub async fn acquire(
        self: &Arc<Self>,
        workspace_key: &str,
        kind: ResourceKind,
        session_id: &str,
        title: &str,
    ) -> Result<ResourceGuard, AcquireError> {
        let kind = normalize_kind(kind);
        let effective_workspace = scope_key_for(workspace_key, &kind, session_id);
        let key = (effective_workspace.clone(), kind.clone());

        let (acquired_immediately, rx_opt) = {
            let mut inner = self.inner.lock();
            reclaim_expired_claim_locked(&mut inner, &key);
            let same_session_held = inner
                .holders
                .get(&key)
                .map(|h| h.session_id == session_id)
                .unwrap_or(false);
            if same_session_held {
                if let Some(holder) = inner.holders.get_mut(&key) {
                    holder.ref_count = holder.ref_count.saturating_add(1);
                    holder.confirmed = true;
                    holder.claim_deadline = None;
                }
                (true, None)
            } else if !inner.holders.contains_key(&key) {
                inner.holders.insert(
                    key.clone(),
                    Holder {
                        session_id: session_id.to_string(),
                        title: title.to_string(),
                        ref_count: 1,
                        confirmed: true,
                        claim_deadline: None,
                    },
                );
                (true, None)
            } else {
                let (tx, rx) = oneshot::channel();
                inner
                    .waiters
                    .entry(key.clone())
                    .or_default()
                    .push_back(Pending {
                        session_id: session_id.to_string(),
                        title: title.to_string(),
                        waker: tx,
                    });
                (false, Some(rx))
            }
        };

        if acquired_immediately {
            let guard = ResourceGuard {
                manager: Arc::clone(self),
                workspace_key: effective_workspace.clone(),
                kind: kind.clone(),
                session_id: session_id.to_string(),
            };
            self.ensure_cross_process_lock(&effective_workspace, &kind)
                .await?;
            return Ok(guard);
        }

        let Some(rx) = rx_opt else {
            self.cancel_waiter(&effective_workspace, &kind, session_id);
            return Err(AcquireError::Shutdown);
        };

        let announced = Arc::new(AtomicBool::new(false));
        let announce_workspace = effective_workspace.clone();
        let announce_kind = kind.clone();
        let announce_sid = session_id.to_string();
        let manager_for_announce = Arc::clone(self);
        let announced_for_task = Arc::clone(&announced);
        let announce_handle = tokio::spawn(async move {
            tokio::time::sleep(WAIT_ANNOUNCE_DELAY).await;
            announced_for_task.store(true, Ordering::SeqCst);
            manager_for_announce.emit_wait_started(
                &announce_workspace,
                &announce_kind,
                &announce_sid,
            );
        });

        let wait_result =
            tokio::time::timeout(ACQUIRE_TIMEOUT, rx).await;

        announce_handle.abort();

        let was_announced = announced.load(Ordering::SeqCst);
        if was_announced {
            self.emit_wait_resolved(&effective_workspace, &kind, session_id);
        }

        match wait_result {
            Ok(Ok(())) => {
                self.confirm_claim(&effective_workspace, &kind, session_id);
                let guard = ResourceGuard {
                    manager: Arc::clone(self),
                    workspace_key: effective_workspace.clone(),
                    kind: kind.clone(),
                    session_id: session_id.to_string(),
                };
                self.ensure_cross_process_lock(&effective_workspace, &kind)
                    .await?;
                Ok(guard)
            }
            Ok(Err(_)) => {
                self.cancel_waiter(&effective_workspace, &kind, session_id);
                Err(AcquireError::Shutdown)
            }
            Err(_) => {
                self.cancel_waiter(&effective_workspace, &kind, session_id);
                Err(AcquireError::Timeout {
                    kind: kind.kind_str(),
                    target: kind.target_str(),
                })
            }
        }
    }

    pub async fn acquire_many(
        self: &Arc<Self>,
        workspace_key: &str,
        kinds: Vec<ResourceKind>,
        session_id: &str,
        title: &str,
    ) -> Result<Vec<ResourceGuard>, AcquireError> {
        let mut kinds: Vec<ResourceKind> = kinds.into_iter().map(normalize_kind).collect();
        kinds.sort_by_key(|a| a.sort_key());
        kinds.dedup();
        let mut guards = Vec::with_capacity(kinds.len());
        for kind in kinds {
            match self.acquire(workspace_key, kind, session_id, title).await {
                Ok(g) => guards.push(g),
                Err(e) => {
                    while let Some(guard) = guards.pop() {
                        drop(guard);
                    }
                    return Err(e);
                }
            }
        }
        Ok(guards)
    }

    pub fn record_read(&self, workspace_key: &str, session_id: &str, path: &Path) {
        let mtime = fs_mtime(path).unwrap_or(SystemTime::UNIX_EPOCH);
        let key = (
            workspace_key.to_string(),
            session_id.to_string(),
            snapshot_path_key(path),
        );
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        match inner.read_snapshots.entry(key) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                e.get_mut().mtime = mtime;
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(SnapshotRecord {
                    mtime,
                    written: false,
                });
                *inner
                    .snapshot_counts
                    .entry((workspace_key.to_string(), session_id.to_string()))
                    .or_insert(0) += 1;
            }
        }
        enforce_snapshot_cap(inner, workspace_key, session_id);
    }

    pub fn record_write(&self, workspace_key: &str, session_id: &str, path: &Path) {
        let mtime = fs_mtime(path).unwrap_or_else(SystemTime::now);
        let snap_key = (
            workspace_key.to_string(),
            session_id.to_string(),
            snapshot_path_key(path),
        );
        let mut inner = self.inner.lock();
        if inner
            .read_snapshots
            .insert(
                snap_key,
                SnapshotRecord {
                    mtime,
                    written: true,
                },
            )
            .is_none()
        {
            *inner
                .snapshot_counts
                .entry((workspace_key.to_string(), session_id.to_string()))
                .or_insert(0) += 1;
        }
        enforce_snapshot_cap(&mut inner, workspace_key, session_id);
    }

    pub fn clear_session_snapshots(&self, session_id: &str) {
        let mut inner = self.inner.lock();
        inner
            .read_snapshots
            .retain(|(_, sid, _), _| sid != session_id);
        inner.snapshot_counts.retain(|(_, sid), _| sid != session_id);
        inner
            .snapshot_evicted_sessions
            .retain(|(_, sid)| sid != session_id);
    }

    pub fn waiters_snapshot_for_session(
        &self,
        workspace_key: &str,
        session_id: &str,
    ) -> Vec<WaiterSnapshot> {
        let inner = self.inner.lock();
        let mut out = Vec::new();
        let workspace_member_prefix = format!("{}::", workspace_key);
        for ((ws, kind), queue) in inner.waiters.iter() {
            let matches_workspace =
                ws == workspace_key || ws.starts_with(&workspace_member_prefix);
            if !matches_workspace {
                continue;
            }
            let mut found = false;
            for pending in queue {
                if pending.session_id == session_id {
                    found = true;
                    break;
                }
            }
            if !found {
                continue;
            }
            let holder = inner.holders.get(&(ws.clone(), kind.clone())).cloned();
            let (holder_session_id, holder_title) = holder
                .map(|h| (h.session_id, h.title))
                .unwrap_or_default();
            out.push(WaiterSnapshot {
                kind: kind.kind_str().to_string(),
                target: kind.target_str(),
                holder_session_id,
                holder_title,
            });
        }
        out
    }

    pub fn has_read(&self, workspace_key: &str, session_id: &str, path: &Path) -> bool {
        let inner = self.inner.lock();
        let snap_key = (
            workspace_key.to_string(),
            session_id.to_string(),
            snapshot_path_key(path),
        );
        inner.read_snapshots.contains_key(&snap_key)
    }

    pub fn is_stale_for(&self, workspace_key: &str, session_id: &str, path: &Path) -> bool {
        let inner = self.inner.lock();
        let snap_key = (
            workspace_key.to_string(),
            session_id.to_string(),
            snapshot_path_key(path),
        );
        let last_seen = inner.read_snapshots.get(&snap_key).copied();
        let evicted_before = last_seen.is_none()
            && inner
                .snapshot_evicted_sessions
                .contains(&(workspace_key.to_string(), session_id.to_string()));
        drop(inner);
        let Some(record) = last_seen else {
            if evicted_before {
                tracing::debug!(
                    path = %path.display(),
                    workspace = %workspace_key,
                    session = %session_id,
                    "stale check has no snapshot for this file and this session experienced snapshot eviction; staleness cannot be detected"
                );
            }
            return false;
        };
        let Some(current) = fs_mtime(path) else {
            return false;
        };
        current > record.mtime
    }

    pub fn cancel_waiters_for_session(self: &Arc<Self>, session_id: &str) {
        let mut inner = self.inner.lock();
        inner.waiters.retain(|_, q| {
            q.retain(|p| p.session_id != session_id);
            !q.is_empty()
        });
    }

    fn cancel_waiter(&self, workspace_key: &str, kind: &ResourceKind, session_id: &str) {
        let key = (workspace_key.to_string(), kind.clone());
        let mut inner = self.inner.lock();
        if let Some(queue) = inner.waiters.get_mut(&key) {
            queue.retain(|p| p.session_id != session_id);
            if queue.is_empty() {
                inner.waiters.remove(&key);
            }
        }
        let orphaned_claim = inner
            .holders
            .get(&key)
            .is_some_and(|h| !h.confirmed && h.session_id == session_id);
        if orphaned_claim {
            inner.holders.remove(&key);
            promote_next_waiter_locked(&mut inner, &key);
        }
        reclaim_expired_claim_locked(&mut inner, &key);
    }

    async fn ensure_cross_process_lock(
        self: &Arc<Self>,
        workspace_key: &str,
        kind: &ResourceKind,
    ) -> Result<(), AcquireError> {
        if !matches!(
            kind,
            ResourceKind::FileWrite { .. } | ResourceKind::WorkspaceExclusive
        ) {
            return Ok(());
        }
        let map_key = (workspace_key.to_string(), kind.clone());
        if self.inner.lock().os_locks.contains_key(&map_key) {
            return Ok(());
        }
        let os_key = format!("{}|{}", kind.kind_str(), workspace_key);
        let deadline = Instant::now() + ACQUIRE_TIMEOUT;
        loop {
            let key_for_task = os_key.clone();
            let attempt = tokio::task::spawn_blocking(move || {
                crate::session::os_lock::OsAdvisoryLock::try_acquire_key(&key_for_task)
            })
            .await;
            match attempt {
                Ok(Ok(Some(lock))) => {
                    self.inner.lock().os_locks.insert(map_key, lock);
                    return Ok(());
                }
                Ok(Ok(None)) => {}
                Ok(Err(err)) => {
                    tracing::warn!(
                        error = %err,
                        key = %os_key,
                        "cross-process advisory lock unavailable; continuing with in-process lock only"
                    );
                    return Ok(());
                }
                Err(_) => return Ok(()),
            }
            if Instant::now() >= deadline {
                return Err(AcquireError::Timeout {
                    kind: kind.kind_str(),
                    target: kind.target_str(),
                });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn confirm_claim(&self, workspace_key: &str, kind: &ResourceKind, session_id: &str) {
        let key = (workspace_key.to_string(), kind.clone());
        let mut inner = self.inner.lock();
        if let Some(h) = inner.holders.get_mut(&key) {
            if h.session_id == session_id {
                h.confirmed = true;
                h.claim_deadline = None;
            }
        }
    }

    fn release(self: &Arc<Self>, workspace_key: &str, kind: &ResourceKind, session_id: &str) {
        let key = (workspace_key.to_string(), kind.clone());
        let mut inner = self.inner.lock();
        let should_remove = match inner.holders.get_mut(&key) {
            Some(h) if h.session_id == session_id => {
                if h.ref_count > 1 {
                    h.ref_count -= 1;
                    return;
                }
                true
            }
            _ => false,
        };
        if !should_remove {
            return;
        }
        inner.holders.remove(&key);
        promote_next_waiter_locked(&mut inner, &key);
        let released_os = if inner.holders.contains_key(&key) {
            None
        } else {
            inner.os_locks.remove(&key)
        };
        drop(inner);
        drop(released_os);
    }

    fn emit_wait_started(
        &self,
        workspace_key: &str,
        kind: &ResourceKind,
        session_id: &str,
    ) {
        let key = (workspace_key.to_string(), kind.clone());
        let holder_info = {
            let inner = self.inner.lock();
            inner.holders.get(&key).cloned()
        };
        let Some(holder) = holder_info else {
            return;
        };
        if holder.session_id == session_id {
            return;
        }
        let _ = self.events.send(ResourceEvent::WaitStarted {
            session_id: session_id.to_string(),
            kind: kind.kind_str().to_string(),
            target: kind.target_str(),
            holder_session_id: holder.session_id,
            holder_title: holder.title,
        });
    }

    fn emit_wait_resolved(
        &self,
        _workspace_key: &str,
        kind: &ResourceKind,
        session_id: &str,
    ) {
        let _ = self.events.send(ResourceEvent::WaitResolved {
            session_id: session_id.to_string(),
            kind: kind.kind_str().to_string(),
            target: kind.target_str(),
        });
    }
}

pub struct ResourceGuard {
    manager: Arc<WorkspaceResourceManager>,
    workspace_key: String,
    kind: ResourceKind,
    session_id: String,
}

impl ResourceGuard {
    pub fn workspace_key(&self) -> &str {
        &self.workspace_key
    }

    pub fn kind(&self) -> &ResourceKind {
        &self.kind
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Drop for ResourceGuard {
    fn drop(&mut self) {
        self.manager.release(&self.workspace_key, &self.kind, &self.session_id);
    }
}

fn reclaim_expired_claim_locked(inner: &mut Inner, key: &(String, ResourceKind)) {
    let stale = inner.holders.get(key).is_some_and(|h| {
        !h.confirmed && h.claim_deadline.is_some_and(|d| Instant::now() > d)
    });
    if stale {
        inner.holders.remove(key);
        promote_next_waiter_locked(inner, key);
    }
}

fn promote_next_waiter_locked(inner: &mut Inner, key: &(String, ResourceKind)) {
    let mut promoted: Option<(String, String)> = None;
    let mut queue_empty = false;
    if let Some(queue) = inner.waiters.get_mut(key) {
        while let Some(next) = queue.pop_front() {
            if next.waker.is_closed() {
                continue;
            }
            let session = next.session_id.clone();
            let title = next.title.clone();
            if next.waker.send(()).is_ok() {
                promoted = Some((session, title));
                break;
            }
        }
        queue_empty = queue.is_empty();
    }
    if let Some((session, title)) = promoted {
        inner.holders.insert(
            key.clone(),
            Holder {
                session_id: session,
                title,
                ref_count: 1,
                confirmed: false,
                claim_deadline: Some(Instant::now() + CLAIM_GRACE),
            },
        );
    }
    if queue_empty {
        inner.waiters.remove(key);
    }
}

fn fs_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn normalize_lock_path(path: &Path) -> String {
    let resolved = crate::util::normalize_path_for_containment(path);
    let s = resolved.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

fn snapshot_path_key(path: &Path) -> PathBuf {
    PathBuf::from(normalize_lock_path(path))
}

fn normalize_kind(kind: ResourceKind) -> ResourceKind {
    match kind {
        ResourceKind::FileWrite { path } => ResourceKind::FileWrite {
            path: snapshot_path_key(&path),
        },
        other => other,
    }
}

fn scope_key_for(workspace_key: &str, kind: &ResourceKind, session_id: &str) -> String {
    match kind {
        ResourceKind::FileWrite { path } => {
            format!("file::{}", normalize_lock_path(path))
        }
        ResourceKind::Browser | ResourceKind::Shell => {
            format!("{}::session::{}", workspace_key, session_id)
        }
        ResourceKind::WorkspaceExclusive => {
            format!("{}::workspace", workspace_key)
        }
    }
}

fn enforce_snapshot_cap(inner: &mut Inner, workspace_key: &str, session_id: &str) {
    let counter_key = (workspace_key.to_string(), session_id.to_string());
    let count = inner
        .snapshot_counts
        .get(&counter_key)
        .copied()
        .unwrap_or(0);
    if count <= MAX_SNAPSHOTS_PER_SESSION {
        return;
    }
    let mut session_entries: Vec<(PathBuf, bool, SystemTime)> = inner
        .read_snapshots
        .iter()
        .filter(|((ws, sid, _), _)| ws == workspace_key && sid == session_id)
        .map(|((_, _, path), record)| (path.clone(), record.written, record.mtime))
        .collect();
    session_entries.sort_by_key(|(_, written, mtime)| (*written, *mtime));
    let evict = session_entries.len().saturating_sub(MAX_SNAPSHOTS_PER_SESSION / 2);
    if evict > 0 {
        inner
            .snapshot_evicted_sessions
            .insert(counter_key.clone());
    }
    for (path, _, _) in session_entries.into_iter().take(evict) {
        inner
            .read_snapshots
            .remove(&(workspace_key.to_string(), session_id.to_string(), path));
    }
    let remaining = inner
        .read_snapshots
        .keys()
        .filter(|(ws, sid, _)| ws == workspace_key && sid == session_id)
        .count();
    inner.snapshot_counts.insert(counter_key, remaining);
}

static GLOBAL_MANAGER: OnceLock<Arc<WorkspaceResourceManager>> = OnceLock::new();

pub fn install_global(manager: Arc<WorkspaceResourceManager>) {
    let _ = GLOBAL_MANAGER.set(manager);
}

pub fn global_workspace_resources() -> Option<Arc<WorkspaceResourceManager>> {
    GLOBAL_MANAGER.get().cloned()
}

tokio::task_local! {
    static SESSION_CONTEXT: SessionContext;
}

#[derive(Clone, Debug)]
pub struct SessionContext {
    pub session_id: String,
    pub workspace_key: String,
    pub title: String,
    pub workspace_dir: String,
    pub connection_id: Option<String>,
}

pub fn current_session_context() -> Option<SessionContext> {
    SESSION_CONTEXT.try_with(|c| c.clone()).ok()
}

pub fn current_connection_id() -> Option<String> {
    SESSION_CONTEXT.try_with(|c| c.connection_id.clone()).ok().flatten()
}

pub async fn scope_session_context<F, R>(ctx: SessionContext, fut: F) -> R
where
    F: std::future::Future<Output = R>,
{
    SESSION_CONTEXT.scope(ctx, fut).await
}

pub fn subagent_session_context(kind: &str, task_id: &str, fallback_workspace_dir: &Path) -> SessionContext {
    let session_id = format!("{kind}-{task_id}");
    let workspace_dir = current_session_context()
        .map(|c| c.workspace_dir)
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| fallback_workspace_dir.to_string_lossy().into_owned());
    SessionContext {
        workspace_key: crate::session::workspace_run::workspace_key_from_path(
            Path::new(&workspace_dir),
            &session_id,
        ),
        title: session_id.clone(),
        workspace_dir,
        connection_id: None,
        session_id,
    }
}

pub async fn acquire_file_write_for_current_session(
    path: &Path,
) -> Option<Result<ResourceGuard, AcquireError>> {
    let ctx = current_session_context()?;
    let manager = global_workspace_resources()?;
    Some(
        manager
            .acquire(
                &ctx.workspace_key,
                ResourceKind::FileWrite {
                    path: path.to_path_buf(),
                },
                &ctx.session_id,
                &ctx.title,
            )
            .await,
    )
}

pub const NO_SESSION_WORKSPACE_KEY: &str = "__no_session__";

static NO_SESSION_ACQUIRER_SEQ: AtomicU64 = AtomicU64::new(0);

fn synthetic_no_session_lock_identity() -> String {
    match tokio::task::try_id() {
        Some(task_id) => format!(
            "{}::{}::task::{}",
            NO_SESSION_WORKSPACE_KEY,
            std::process::id(),
            task_id
        ),
        None => format!(
            "{}::{}::seq::{}",
            NO_SESSION_WORKSPACE_KEY,
            std::process::id(),
            NO_SESSION_ACQUIRER_SEQ.fetch_add(1, Ordering::Relaxed)
        ),
    }
}

fn lock_identity() -> (String, String, String) {
    match current_session_context() {
        Some(ctx) => (ctx.workspace_key, ctx.session_id, ctx.title),
        None => (
            NO_SESSION_WORKSPACE_KEY.to_string(),
            synthetic_no_session_lock_identity(),
            "no-session".to_string(),
        ),
    }
}

pub async fn acquire_file_write_locked(
    path: &Path,
) -> Option<Result<ResourceGuard, AcquireError>> {
    let manager = global_workspace_resources()?;
    let (workspace_key, session_id, title) = lock_identity();
    Some(
        manager
            .acquire(
                &workspace_key,
                ResourceKind::FileWrite {
                    path: path.to_path_buf(),
                },
                &session_id,
                &title,
            )
            .await,
    )
}

pub async fn acquire_file_write_guard(
    path: &Path,
) -> Result<Option<ResourceGuard>, AcquireError> {
    match acquire_file_write_locked(path).await {
        Some(Ok(guard)) => Ok(Some(guard)),
        Some(Err(e)) => Err(e),
        None => {
            tracing::warn!(
                path = %path.display(),
                "no workspace resource manager installed; proceeding to write without a file write lock"
            );
            Ok(None)
        }
    }
}

pub async fn acquire_many_file_write_guards(
    paths: Vec<PathBuf>,
) -> Result<Option<Vec<ResourceGuard>>, AcquireError> {
    let total = paths.len();
    let described: String = paths
        .iter()
        .take(8)
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    match acquire_many_file_writes_locked(paths).await {
        Some(Ok(guards)) => Ok(Some(guards)),
        Some(Err(e)) => Err(e),
        None => {
            tracing::warn!(
                paths = %described,
                total,
                "no workspace resource manager installed; proceeding to write without file write locks"
            );
            Ok(None)
        }
    }
}

pub async fn acquire_many_file_writes_locked(
    paths: Vec<PathBuf>,
) -> Option<Result<Vec<ResourceGuard>, AcquireError>> {
    let manager = global_workspace_resources()?;
    let (workspace_key, session_id, title) = lock_identity();
    let kinds = paths
        .into_iter()
        .map(|p| ResourceKind::FileWrite { path: p })
        .collect();
    Some(
        manager
            .acquire_many(&workspace_key, kinds, &session_id, &title)
            .await,
    )
}

pub async fn acquire_shell_for_current_session() -> Option<Result<ResourceGuard, AcquireError>> {
    let ctx = current_session_context()?;
    let manager = global_workspace_resources()?;
    Some(
        manager
            .acquire(
                &ctx.workspace_key,
                ResourceKind::Shell,
                &ctx.session_id,
                &ctx.title,
            )
            .await,
    )
}

pub async fn acquire_browser_for_current_session() -> Option<Result<ResourceGuard, AcquireError>> {
    let ctx = current_session_context()?;
    let manager = global_workspace_resources()?;
    Some(
        manager
            .acquire(
                &ctx.workspace_key,
                ResourceKind::Browser,
                &ctx.session_id,
                &ctx.title,
            )
            .await,
    )
}

pub async fn acquire_workspace_exclusive_for_current_session(
) -> Option<Result<ResourceGuard, AcquireError>> {
    let ctx = current_session_context()?;
    let manager = global_workspace_resources()?;
    Some(
        manager
            .acquire(
                &ctx.workspace_key,
                ResourceKind::WorkspaceExclusive,
                &ctx.session_id,
                &ctx.title,
            )
            .await,
    )
}

fn snapshot_identity() -> (String, String) {
    match current_session_context() {
        Some(ctx) => (ctx.workspace_key, ctx.session_id),
        None => (
            NO_SESSION_WORKSPACE_KEY.to_string(),
            NO_SESSION_WORKSPACE_KEY.to_string(),
        ),
    }
}

pub fn record_read_for_current_session(path: &Path) {
    let Some(manager) = global_workspace_resources() else {
        return;
    };
    let (workspace_key, session_id) = snapshot_identity();
    manager.record_read(&workspace_key, &session_id, path);
}

pub fn record_write_for_current_session(path: &Path) {
    crate::agent::designer::record_artifact_if_designer(path);
    let Some(manager) = global_workspace_resources() else {
        return;
    };
    let (workspace_key, session_id) = snapshot_identity();
    manager.record_write(&workspace_key, &session_id, path);
}

pub fn is_stale_for_current_session(path: &Path) -> bool {
    let Some(manager) = global_workspace_resources() else {
        return false;
    };
    let (workspace_key, session_id) = snapshot_identity();
    manager.is_stale_for(&workspace_key, &session_id, path)
}

pub fn has_read_in_current_session(path: &Path) -> bool {
    let Some(manager) = global_workspace_resources() else {
        return true;
    };
    let (workspace_key, session_id) = snapshot_identity();
    manager.has_read(&workspace_key, &session_id, path)
}

pub fn stale_file_error_message(path: &Path) -> String {
    format!(
        "FILE_MODIFIED_BY_OTHER_SESSION: The file `{}` was modified by another agent session \
         while you were waiting for the write lock. Re-read the file with the file_read tool \
         before applying any edits.",
        path.display()
    )
}
