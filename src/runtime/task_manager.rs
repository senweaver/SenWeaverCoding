// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::future::Future;
use std::time::Instant;

use futures_util::FutureExt;
use parking_lot::Mutex;
use tokio::task::{AbortHandle, JoinHandle};
use tracing::{Instrument, info_span};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TaskInfo {

    pub id: String,

    pub name: String,

    pub spawned_at: Instant,

    pub abort_handle: AbortHandle,
}

static REGISTRY: std::sync::LazyLock<Mutex<Vec<TaskInfo>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

pub struct TaskHandle {

    inner: Option<JoinHandle<()>>,
}

impl TaskHandle {

    pub fn into_inner(mut self) -> JoinHandle<()> {
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

struct RegistryCleanup {
    id: String,
}

impl Drop for RegistryCleanup {
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

    let span = info_span!("task", name = %name, task_id = %id);
    let registry_id = id.clone();
    let task_name = name.clone();

    let wrapped = async move {
        let _cleanup = RegistryCleanup { id: registry_id };
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
    };

    let inner = tokio::spawn(wrapped.instrument(span));
    let abort_handle = inner.abort_handle();
    let info = TaskInfo {
        id,
        name,
        spawned_at: Instant::now(),
        abort_handle,
    };
    REGISTRY.lock().push(info);

    TaskHandle { inner: Some(inner) }
}

pub fn spawn_supervised_restartable<F, Fut>(
    name: impl Into<String>,
    max_restarts: usize,
    factory: F,
) -> TaskHandle
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    let name: String = name.into();
    let loop_name = name.clone();
    spawn_supervised(name, async move {
        let mut attempts = 0usize;
        loop {
            match std::panic::AssertUnwindSafe(factory()).catch_unwind().await {
                Ok(_) => break,
                Err(payload) => {
                    let msg = panic_message(&payload);
                    attempts += 1;
                    if attempts > max_restarts {
                        tracing::error!(
                            target: "task.panic",
                            task = %loop_name,
                            panic = %msg,
                            restarts = attempts - 1,
                            "supervised task panicked; restart budget exhausted"
                        );
                        break;
                    }
                    tracing::error!(
                        target: "task.panic",
                        task = %loop_name,
                        panic = %msg,
                        attempt = attempts,
                        max_restarts,
                        "supervised task panicked; restarting"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(
                        200 * attempts as u64,
                    ))
                    .await;
                }
            }
        }
    })
}

pub fn abort_all() -> usize {
    let infos: Vec<TaskInfo> = {
        let mut guard = REGISTRY.lock();
        std::mem::take(&mut *guard)
    };
    let count = infos.len();
    for info in infos {
        info.abort_handle.abort();
    }
    count
}

pub fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
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

static PROCESS_START_AT: std::sync::OnceLock<chrono::DateTime<chrono::Utc>> =
    std::sync::OnceLock::new();
static PROCESS_START_INSTANT: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn ensure_process_start_recorded() {
    PROCESS_START_AT.get_or_init(chrono::Utc::now);
    PROCESS_START_INSTANT.get_or_init(Instant::now);
}

pub fn process_started_at() -> chrono::DateTime<chrono::Utc> {
    *PROCESS_START_AT.get_or_init(chrono::Utc::now)
}

pub fn process_uptime_secs() -> u64 {
    PROCESS_START_INSTANT
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs()
}
