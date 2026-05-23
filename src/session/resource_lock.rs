// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::{broadcast, oneshot};

const EVENT_CHANNEL_CAP: usize = 512;
const WAIT_ANNOUNCE_DELAY: Duration = Duration::from_millis(50);
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_SNAPSHOTS_PER_SESSION: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ResourceKind {
    FileWrite { path: PathBuf },
    Browser,
    Shell,
}

impl ResourceKind {
    pub fn kind_str(&self) -> &'static str {
        match self {
            ResourceKind::FileWrite { .. } => "file",
            ResourceKind::Browser => "browser",
            ResourceKind::Shell => "shell",
        }
    }

    pub fn target_str(&self) -> String {
        match self {
            ResourceKind::FileWrite { path } => path.to_string_lossy().to_string(),
            ResourceKind::Browser | ResourceKind::Shell => String::new(),
        }
    }

    fn sort_key(&self) -> (u8, String) {
        match self {
            ResourceKind::FileWrite { path } => (0, path.to_string_lossy().to_string()),
            ResourceKind::Browser => (1, String::new()),
            ResourceKind::Shell => (2, String::new()),
        }
    }
}

#[derive(Clone, Debug)]
struct Holder {
    session_id: String,
    title: String,
    ref_count: usize,
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

#[derive(Default)]
struct Inner {
    holders: HashMap<(String, ResourceKind), Holder>,
    waiters: HashMap<(String, ResourceKind), VecDeque<Pending>>,
    read_snapshots: HashMap<(String, String, PathBuf), SystemTime>,
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
        let effective_workspace = scope_key_for(workspace_key, &kind, session_id);
        let key = (effective_workspace.clone(), kind.clone());

        let (acquired_immediately, rx_opt) = {
            let mut inner = self.inner.lock();
            let same_session_held = inner
                .holders
                .get(&key)
                .map(|h| h.session_id == session_id)
                .unwrap_or(false);
            if same_session_held {
                if let Some(holder) = inner.holders.get_mut(&key) {
                    holder.ref_count = holder.ref_count.saturating_add(1);
                }
                (true, None)
            } else if !inner.holders.contains_key(&key) {
                inner.holders.insert(
                    key.clone(),
                    Holder {
                        session_id: session_id.to_string(),
                        title: title.to_string(),
                        ref_count: 1,
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
            return Ok(ResourceGuard {
                manager: Arc::clone(self),
                workspace_key: effective_workspace,
                kind,
                session_id: session_id.to_string(),
            });
        }

        let rx = rx_opt.expect("waiter must have receiver when not acquired");

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
            Ok(Ok(())) => Ok(ResourceGuard {
                manager: Arc::clone(self),
                workspace_key: effective_workspace,
                kind,
                session_id: session_id.to_string(),
            }),
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
        mut kinds: Vec<ResourceKind>,
        session_id: &str,
        title: &str,
    ) -> Result<Vec<ResourceGuard>, AcquireError> {
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
            path.to_path_buf(),
        );
        let mut inner = self.inner.lock();
        inner.read_snapshots.insert(key, mtime);
        enforce_snapshot_cap(&mut inner, workspace_key, session_id);
    }

    pub fn record_write(&self, workspace_key: &str, session_id: &str, path: &Path) {
        let mtime = fs_mtime(path).unwrap_or_else(SystemTime::now);
        let snap_key = (
            workspace_key.to_string(),
            session_id.to_string(),
            path.to_path_buf(),
        );
        let mut inner = self.inner.lock();
        inner.read_snapshots.insert(snap_key, mtime);
        enforce_snapshot_cap(&mut inner, workspace_key, session_id);
    }

    pub fn waiters_snapshot_for_session(
        &self,
        workspace_key: &str,
        session_id: &str,
    ) -> Vec<WaiterSnapshot> {
        let inner = self.inner.lock();
        let mut out = Vec::new();
        let session_prefix = format!("{}::session::", workspace_key);
        for ((ws, kind), queue) in inner.waiters.iter() {
            let matches_workspace = ws == workspace_key || ws.starts_with(&session_prefix);
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

    pub fn is_stale_for(&self, workspace_key: &str, session_id: &str, path: &Path) -> bool {
        let inner = self.inner.lock();
        let snap_key = (
            workspace_key.to_string(),
            session_id.to_string(),
            path.to_path_buf(),
        );
        let last_seen = inner.read_snapshots.get(&snap_key).copied();
        drop(inner);
        let Some(last_seen) = last_seen else {
            return false;
        };
        let Some(current) = fs_mtime(path) else {
            return false;
        };
        current > last_seen
    }

    pub fn release_all_for_session(self: &Arc<Self>, session_id: &str) {
        let mut to_promote: Vec<((String, ResourceKind), Pending)> = Vec::new();
        {
            let mut inner = self.inner.lock();
            let mut released_keys: Vec<(String, ResourceKind)> = Vec::new();
            for (key, holder) in inner.holders.iter() {
                if holder.session_id == session_id {
                    released_keys.push(key.clone());
                }
            }
            for key in released_keys {
                inner.holders.remove(&key);
                let mut chosen: Option<Pending> = None;
                let mut queue_empty = false;
                if let Some(queue) = inner.waiters.get_mut(&key) {
                    while let Some(next) = queue.pop_front() {
                        if next.waker.is_closed() {
                            continue;
                        }
                        chosen = Some(next);
                        break;
                    }
                    queue_empty = queue.is_empty();
                }
                if let Some(pending) = chosen {
                    inner.holders.insert(
                        key.clone(),
                        Holder {
                            session_id: pending.session_id.clone(),
                            title: pending.title.clone(),
                            ref_count: 1,
                        },
                    );
                    to_promote.push((key.clone(), pending));
                }
                if queue_empty {
                    inner.waiters.remove(&key);
                }
            }

            inner
                .waiters
                .retain(|_, q| {
                    q.retain(|p| p.session_id != session_id);
                    !q.is_empty()
                });
        }
        for (_, pending) in to_promote {
            let _ = pending.waker.send(());
        }
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
    }

    fn release(self: &Arc<Self>, workspace_key: &str, kind: &ResourceKind, session_id: &str) {
        let key = (workspace_key.to_string(), kind.clone());
        let next_pending = {
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

            let mut promoted: Option<Pending> = None;
            let mut queue_empty = false;
            if let Some(queue) = inner.waiters.get_mut(&key) {
                while let Some(next) = queue.pop_front() {
                    if next.waker.is_closed() {
                        continue;
                    }
                    promoted = Some(next);
                    break;
                }
                queue_empty = queue.is_empty();
            }
            if let Some(ref pending) = promoted {
                inner.holders.insert(
                    key.clone(),
                    Holder {
                        session_id: pending.session_id.clone(),
                        title: pending.title.clone(),
                        ref_count: 1,
                    },
                );
            }
            if queue_empty {
                inner.waiters.remove(&key);
            }
            promoted
        };
        if let Some(pending) = next_pending {
            let _ = pending.waker.send(());
        }
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

fn fs_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn scope_key_for(workspace_key: &str, kind: &ResourceKind, session_id: &str) -> String {
    match kind {
        ResourceKind::FileWrite { .. } => workspace_key.to_string(),
        ResourceKind::Browser | ResourceKind::Shell => {
            format!("{}::session::{}", workspace_key, session_id)
        }
    }
}

fn enforce_snapshot_cap(inner: &mut Inner, workspace_key: &str, session_id: &str) {
    let count = inner
        .read_snapshots
        .keys()
        .filter(|(ws, sid, _)| ws == workspace_key && sid == session_id)
        .count();
    if count > MAX_SNAPSHOTS_PER_SESSION {
        inner
            .read_snapshots
            .retain(|(ws, sid, _), _| !(ws == workspace_key && sid == session_id));
    }
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
}

pub fn current_session_context() -> Option<SessionContext> {
    SESSION_CONTEXT.try_with(|c| c.clone()).ok()
}

pub async fn scope_session_context<F, R>(ctx: SessionContext, fut: F) -> R
where
    F: std::future::Future<Output = R>,
{
    SESSION_CONTEXT.scope(ctx, fut).await
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

pub async fn acquire_many_file_writes_for_current_session(
    paths: Vec<PathBuf>,
) -> Option<Result<Vec<ResourceGuard>, AcquireError>> {
    let ctx = current_session_context()?;
    let manager = global_workspace_resources()?;
    let kinds = paths
        .into_iter()
        .map(|p| ResourceKind::FileWrite { path: p })
        .collect();
    Some(
        manager
            .acquire_many(&ctx.workspace_key, kinds, &ctx.session_id, &ctx.title)
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

pub fn record_read_for_current_session(path: &Path) {
    let Some(ctx) = current_session_context() else {
        return;
    };
    let Some(manager) = global_workspace_resources() else {
        return;
    };
    manager.record_read(&ctx.workspace_key, &ctx.session_id, path);
}

pub fn record_write_for_current_session(path: &Path) {
    let Some(ctx) = current_session_context() else {
        return;
    };
    let Some(manager) = global_workspace_resources() else {
        return;
    };
    manager.record_write(&ctx.workspace_key, &ctx.session_id, path);
}

pub fn is_stale_for_current_session(path: &Path) -> bool {
    let Some(ctx) = current_session_context() else {
        return false;
    };
    let Some(manager) = global_workspace_resources() else {
        return false;
    };
    manager.is_stale_for(&ctx.workspace_key, &ctx.session_id, path)
}

pub fn stale_file_error_message(path: &Path) -> String {
    format!(
        "FILE_MODIFIED_BY_OTHER_SESSION: The file `{}` was modified by another agent session \
         while you were waiting for the write lock. Re-read the file with the file_read tool \
         before applying any edits.",
        path.display()
    )
}
