// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::FutureExt;
use parking_lot::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info_span, warn};

const SCHEDULER_RUN_MAX_SECS: u64 = 3 * 60 * 60;
const SCHEDULER_CANCEL_GRACE_SECS: u64 = 30;

use super::core::{SchedulableTask, SchedulerEvent, TaskOutcome, TaskScheduler};
use crate::observability::runtime_trace::{AgentSpanContext, record_event_with_ctx};
use crate::observability::scheduler_metrics::{
    self, TaskTerminalStatus, add_worker_busy_nanos,
};

pub type TaskExecutor = Arc<
    dyn for<'a> Fn(
            &'a SchedulableTask,
            CancellationToken,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, Default)]
pub struct SchedulerSpanContext {

    pub parent_agent_id: Option<String>,

    pub delegation_id: Option<String>,
}

impl SchedulerSpanContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_parent_agent(mut self, id: impl Into<String>) -> Self {
        self.parent_agent_id = Some(id.into());
        self
    }

    pub fn with_delegation(mut self, id: impl Into<String>) -> Self {
        self.delegation_id = Some(id.into());
        self
    }
}

pub struct TaskSchedulerRuntime {
    scheduler: Arc<Mutex<TaskScheduler>>,
    semaphore: Arc<Semaphore>,
    cancellation: CancellationToken,
    worker_count: usize,
}

impl TaskSchedulerRuntime {

    pub fn new(scheduler: TaskScheduler) -> Self {
        let max_parallel = scheduler.max_parallel();
        let cancellation = scheduler.cancellation_token();
        Self {
            scheduler: Arc::new(Mutex::new(scheduler)),
            semaphore: Arc::new(Semaphore::new(max_parallel)),
            cancellation,
            worker_count: max_parallel,
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub async fn run(&self, executor: TaskExecutor) -> Vec<TaskOutcome> {
        self.run_with_context(executor, SchedulerSpanContext::default())
            .await
    }

    pub async fn run_with_context(
        &self,
        executor: TaskExecutor,
        ctx: SchedulerSpanContext,
    ) -> Vec<TaskOutcome> {
        let ctx = Arc::new(ctx);

        let mut handles = Vec::with_capacity(self.worker_count);
        for worker_idx in 0..self.worker_count {
            let scheduler = self.scheduler.clone();
            let semaphore = self.semaphore.clone();
            let cancellation = self.cancellation.clone();
            let executor = executor.clone();
            let ctx = ctx.clone();
            let handle = crate::runtime::spawn_supervised(
                format!("agent.scheduler.worker.{worker_idx}"),
                async move {
                    worker_loop(
                        worker_idx,
                        scheduler,
                        semaphore,
                        cancellation,
                        executor,
                        ctx,
                    )
                    .await
                },
            );
            handles.push(handle);
        }

        let join_handles: Vec<tokio::task::JoinHandle<()>> = handles
            .into_iter()
            .map(crate::runtime::TaskHandle::into_inner)
            .collect();
        match crate::runtime::task_manager::shutdown_with_grace(
            Some(&self.cancellation),
            join_handles,
            Some(Duration::from_secs(SCHEDULER_RUN_MAX_SECS)),
            Duration::from_secs(SCHEDULER_CANCEL_GRACE_SECS),
        )
        .await
        {
            crate::runtime::task_manager::ShutdownOutcome::Completed => {}
            crate::runtime::task_manager::ShutdownOutcome::CancelledGraceful => {
                warn!(
                    max_secs = SCHEDULER_RUN_MAX_SECS,
                    "scheduler run exceeded absolute deadline; workers stopped within the \
                     cancellation grace period, returning partial outcomes"
                );
            }
            crate::runtime::task_manager::ShutdownOutcome::Aborted => {
                warn!(
                    max_secs = SCHEDULER_RUN_MAX_SECS,
                    grace_secs = SCHEDULER_CANCEL_GRACE_SECS,
                    "scheduler run exceeded absolute deadline and workers did not stop within \
                     the cancellation grace period; workers hard-aborted, returning partial outcomes"
                );
            }
        }

        {
            let mut sched = self.scheduler.lock();
            if !sched.is_finished() {
                let synthesized = sched.synthesize_missing_outcomes(
                    "scheduler run ended before this task reached a terminal state",
                );
                if synthesized > 0 {
                    warn!(
                        synthesized,
                        "synthesized failure outcomes for tasks left non-terminal by the run"
                    );
                }
            }
        }

        self.scheduler.lock().outcomes()
    }
}

impl Drop for TaskSchedulerRuntime {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

async fn worker_loop(
    worker_idx: usize,
    scheduler: Arc<Mutex<TaskScheduler>>,
    semaphore: Arc<Semaphore>,
    cancellation: CancellationToken,
    executor: TaskExecutor,
    ctx: Arc<SchedulerSpanContext>,
) {
    let mut rx = scheduler.lock().subscribe();

    drain_ready_by_priority(
        worker_idx,
        &scheduler,
        &semaphore,
        &cancellation,
        &executor,
        &ctx,
    )
    .await;

    loop {
        if cancellation.is_cancelled() {
            debug!(worker_idx, "scheduler worker cancelled");
            return;
        }
        if scheduler.lock().is_finished() {
            return;
        }

        let evt = tokio::select! {
            res = rx.recv() => res,
            _ = cancellation.cancelled() => return,
        };

        match evt {
            Ok(SchedulerEvent::TaskReady { .. }) => {
                drain_ready_by_priority(
                    worker_idx,
                    &scheduler,
                    &semaphore,
                    &cancellation,
                    &executor,
                    &ctx,
                )
                .await;
            }
            Ok(SchedulerEvent::GraphCompleted) => {
                return;
            }
            Ok(SchedulerEvent::TaskCompleted { .. })
            | Ok(SchedulerEvent::TaskFailed { .. })
            | Ok(SchedulerEvent::TaskCancelled { .. }) => {

            }
            Err(RecvError::Closed) => return,
            Err(RecvError::Lagged(n)) => {
                scheduler_metrics::incr_broadcast_lagged(n);
                drain_ready_by_priority(
                    worker_idx,
                    &scheduler,
                    &semaphore,
                    &cancellation,
                    &executor,
                    &ctx,
                )
                .await;
            }
        }
    }
}

async fn drain_ready_by_priority(
    worker_idx: usize,
    scheduler: &Arc<Mutex<TaskScheduler>>,
    semaphore: &Arc<Semaphore>,
    cancellation: &CancellationToken,
    executor: &TaskExecutor,
    ctx: &Arc<SchedulerSpanContext>,
) {
    loop {
        if cancellation.is_cancelled() {
            return;
        }
        let claimed = scheduler.lock().claim_next();
        let Some(task) = claimed else {
            return;
        };
        execute_claimed(
            worker_idx,
            scheduler,
            semaphore,
            cancellation,
            executor,
            ctx,
            task,
        )
        .await;
    }
}

async fn execute_claimed(
    worker_idx: usize,
    scheduler: &Arc<Mutex<TaskScheduler>>,
    semaphore: &Arc<Semaphore>,
    cancellation: &CancellationToken,
    executor: &TaskExecutor,
    ctx: &Arc<SchedulerSpanContext>,
    task: SchedulableTask,
) {
    let permit = match semaphore.clone().acquire_owned().await {
        Ok(p) => p,
        Err(_) => return,
    };

    let span = info_span!(
        "scheduler.task",
        task_id = %task.id,
        parent_agent_id = ctx.parent_agent_id.as_deref().unwrap_or(""),
        delegation_id = ctx.delegation_id.as_deref().unwrap_or(""),
        capability = %task.required_capability,
    );

    let task_id = task.id.clone();
    let fut = async {
        let _permit = permit;

        let busy_start = Instant::now();

        record_event_with_ctx(
            "scheduler.task_started",
            None,
            None,
            None,
            None,
            None,
            Some(&task.description),
            serde_json::json!({
                "capability": &task.required_capability,
                "depends_on": &task.depends_on,
            }),
            AgentSpanContext {
                parent_agent_id: ctx.parent_agent_id.as_deref(),
                agent_id: None,
                task_id: Some(&task_id),
                delegation_id: ctx.delegation_id.as_deref(),
            },
        );

        let result = match std::panic::AssertUnwindSafe(executor(&task, cancellation.child_token()))
            .catch_unwind()
            .await
        {
            Ok(r) => r,
            Err(payload) => {
                let msg = crate::runtime::panic_message(&payload);
                warn!(task_id = %task_id, panic = %msg, "task executor panicked");
                Err(format!("task executor panicked: {msg}"))
            }
        };
        let elapsed = busy_start.elapsed();
        add_worker_busy_nanos(worker_idx, elapsed.as_nanos().min(u128::from(u64::MAX)) as u64);

        match result {
            Ok(output) => {
                scheduler.lock().complete(&task_id, output.clone());
                scheduler_metrics::record_task_duration_ms(
                    TaskTerminalStatus::Succeeded,
                    duration_to_ms(elapsed),
                );
                record_event_with_ctx(
                    "scheduler.task_completed",
                    None,
                    None,
                    None,
                    None,
                    Some(true),
                    None,
                    serde_json::json!({
                        "output_preview": output.chars().take(200).collect::<String>(),
                    }),
                    AgentSpanContext {
                        parent_agent_id: ctx.parent_agent_id.as_deref(),
                        agent_id: None,
                        task_id: Some(&task_id),
                        delegation_id: ctx.delegation_id.as_deref(),
                    },
                );
            }
            Err(err) => {
                warn!(task_id = %task_id, error = %err, "Task failed  -  cascading to dependents");
                scheduler.lock().fail(&task_id, err.clone());
                scheduler_metrics::record_task_duration_ms(
                    TaskTerminalStatus::Failed,
                    duration_to_ms(elapsed),
                );
                record_event_with_ctx(
                    "scheduler.task_failed",
                    None,
                    None,
                    None,
                    None,
                    Some(false),
                    Some(&err),
                    serde_json::Value::Null,
                    AgentSpanContext {
                        parent_agent_id: ctx.parent_agent_id.as_deref(),
                        agent_id: None,
                        task_id: Some(&task_id),
                        delegation_id: ctx.delegation_id.as_deref(),
                    },
                );
            }
        }
    };

    fut.instrument(span).await;
}

fn duration_to_ms(d: Duration) -> u64 {
    d.as_millis().min(u128::from(u64::MAX)) as u64
}
