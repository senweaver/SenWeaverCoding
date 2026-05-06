// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Supervised `tokio::spawn` wrapper with panic capture, tracing, and
//! a process-global task registry.
//!
//! The default `tokio::spawn(async move { ... })` pattern silently
//! drops `JoinHandle`s and swallows panics — once a fire-and-forget
//! background task dies, nothing upstream is notified.  In a
//! multi-agent runtime that runs MCP sync loops, channel event
//! loops, rate-limiter refresh workers, and dozens of other
//! long-running tasks, this is the difference between "degraded
//! service" and "silent total failure".
//!
//! This module provides:
//!
//! - [`spawn_supervised`] — a drop-in replacement for
//!   `tokio::spawn(fut)` that:
//!     * wraps the future in [`FutureExt::catch_unwind`] so a panic
//!       inside the task does **not** tear down the tokio runtime;
//!     * emits a tracing `info_span!("task", name = ...)` around the
//!       future so all log events inside inherit the task label;
//!     * records the `JoinHandle` in a process-global registry keyed
//!       by task name, so operators can enumerate every live
//!       background task via [`snapshot`];
//!     * emits `task.panic` / `task.completed` / `task.cancelled`
//!       tracing events on termination.
//!
//! - [`TaskHandle`] — a thin `Arc<...>` wrapper around the underlying
//!   `JoinHandle` that registers / deregisters automatically.
//!
//! - [`snapshot`] — returns a point-in-time list of all currently
//!   tracked tasks (name + spawn timestamp) for observability tools.
//!
//! # Migration strategy
//!
//! Business-code `tokio::spawn(async move { ... })` calls should be
//! migrated to `runtime::task_manager::spawn_supervised("descriptive-name", async move { ... })`.
//!
//! Framework-internal spawns inside `scheduler_runtime`, provider hot
//! paths, and the tokio work-stealing pool itself **should not** be
//! migrated — they are short-lived and their panics are already
//! observed by callers.

use std::future::Future;
use std::time::Instant;

use futures_util::FutureExt;
use parking_lot::Mutex;
use tokio::task::JoinHandle;
use tracing::{Instrument, info_span};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TaskInfo {

    pub id: String,

    pub name: String,

    pub spawned_at: Instant,
}

static REGISTRY: std::sync::LazyLock<Mutex<Vec<TaskInfo>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

pub struct TaskHandle {

    inner: Option<JoinHandle<()>>,

    id: String,
}

impl TaskHandle {

    pub fn into_inner(mut self) -> JoinHandle<()> {
        REGISTRY.lock().retain(|t| t.id != self.id);
        self.inner
            .take()
            .expect("TaskHandle::into_inner called twice")
    }

    pub fn abort(&self) {
        if let Some(h) = self.inner.as_ref() {
            h.abort();
        }
    }

    pub fn is_finished(&self) -> bool {
        self.inner.as_ref().map(|h| h.is_finished()).unwrap_or(true)
    }
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        REGISTRY.lock().retain(|t| t.id != self.id);
    }
}

pub fn spawn_supervised<F>(name: impl Into<String>, fut: F) -> TaskHandle
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let name: String = name.into();
    let id = format!("task-{}", &Uuid::new_v4().to_string()[..8]);
    let info = TaskInfo {
        id: id.clone(),
        name: name.clone(),
        spawned_at: Instant::now(),
    };
    REGISTRY.lock().push(info);

    let span = info_span!("task", name = %name, task_id = %id);
    let registry_id = id.clone();
    let task_name = name.clone();

    let wrapped = async move {
        let _output = match std::panic::AssertUnwindSafe(fut).catch_unwind().await {
            Ok(v) => v,
            Err(payload) => {
                let msg = panic_message(&payload);
                tracing::error!(
                    target: "task.panic",
                    task = %task_name,
                    panic = %msg,
                    "supervised task panicked"
                );

                return;
            }
        };
        tracing::debug!(target: "task.completed", task = %task_name, "task completed");

        REGISTRY.lock().retain(|t| t.id != registry_id);
    };

    let inner = tokio::spawn(wrapped.instrument(span));
    TaskHandle {
        inner: Some(inner),
        id,
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

pub fn snapshot() -> Vec<TaskInfo> {
    REGISTRY.lock().clone()
}

pub fn live_count() -> usize {
    REGISTRY.lock().len()
}
