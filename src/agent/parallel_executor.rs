// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossbeam_deque::{Injector, Steal, Stealer, Worker as DequeWorker};
use futures_util::FutureExt;
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{Notify, Semaphore, oneshot};
use tokio_util::sync::CancellationToken;

use crate::error::{SchedulerError, SenError};
use crate::observability::scheduler_metrics;

pub type BoxedTaskFuture = Pin<Box<dyn Future<Output = TaskOutput> + Send + 'static>>;

pub type TaskFutureFactory = Box<dyn FnOnce() -> BoxedTaskFuture + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priority(u8);

impl Priority {
    pub const CRITICAL: Priority = Priority(4);
    pub const HIGH: Priority = Priority(3);
    pub const NORMAL: Priority = Priority(2);
    pub const LOW: Priority = Priority(1);
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {

        self.0.cmp(&other.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskHandle {
    pub id: u64,
}

impl TaskHandle {
    pub fn new(id: u64) -> Self {
        Self { id }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum AggregationStrategy {

    #[default]
    FirstComplete,

    AllComplete,

    BestOfN { n: usize },

    VotingConsensus { threshold: f64 },

    Cheapest { budget_tokens: u64 },

    Fastest { deadline_ms: u64 },
}

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub max_concurrent: usize,
    pub queue_capacity: usize,
    pub default_timeout_secs: Option<u64>,
    pub fairness_enabled: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            queue_capacity: 1024,
            default_timeout_secs: Some(300),
            fairness_enabled: true,
        }
    }
}

struct QueuedTask {
    task_id: u64,
    handle: TaskHandle,
    timeout: Duration,
    future_factory: TaskFutureFactory,
    result: Arc<Mutex<Option<TaskOutput>>>,
}

struct TaskEntry {
    result: Arc<Mutex<Option<TaskOutput>>>,
    done_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[derive(Debug, Clone)]
pub struct TaskOutput {
    pub handle: TaskHandle,
    pub output: Value,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub metadata: HashMap<String, Value>,
}

impl TaskOutput {
    pub fn success(handle: TaskHandle, output: Value, duration_ms: u64) -> Self {
        Self {
            handle,
            output,
            success: true,
            error: None,
            duration_ms,
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct AggregatedOutput {
    pub succeeded: Vec<TaskOutput>,
    pub failed: Vec<TaskFailure>,
    pub elapsed_ms: u64,
}

impl AggregatedOutput {

    pub fn all_ok(&self) -> bool {
        self.failed.is_empty()
    }

    pub fn total(&self) -> usize {
        self.succeeded.len() + self.failed.len()
    }

    pub fn into_successes(self) -> Vec<TaskOutput> {
        self.succeeded
    }
}

#[derive(Debug, Clone)]
pub struct TaskFailure {
    pub handle: TaskHandle,
    pub reason: FailureReason,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub enum FailureReason {
    Timeout,
    Panic(String),
    Application(String),
    Cancelled,
    Internal(String),
}

impl FailureReason {
    pub fn kind_tag(&self) -> &'static str {
        match self {
            FailureReason::Timeout => "timeout",
            FailureReason::Panic(_) => "panic",
            FailureReason::Application(_) => "application",
            FailureReason::Cancelled => "cancelled",
            FailureReason::Internal(_) => "internal",
        }
    }
}

#[derive(Default)]
pub struct ExecutorMetrics {
    pub spawned: AtomicU64,
    pub completed: AtomicU64,
    pub panicked: AtomicU64,
    pub timed_out: AtomicU64,
    pub cancelled: AtomicU64,
    pub queue_high_water: AtomicU64,
    pub stolen: AtomicU64,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutorMetricsSnapshot {
    pub spawned: u64,
    pub completed: u64,
    pub panicked: u64,
    pub timed_out: u64,
    pub cancelled: u64,
    pub queue_high_water: u64,
    pub stolen: u64,
}

impl ExecutorMetrics {
    pub fn snapshot(&self) -> ExecutorMetricsSnapshot {
        ExecutorMetricsSnapshot {
            spawned: self.spawned.load(Ordering::Relaxed),
            completed: self.completed.load(Ordering::Relaxed),
            panicked: self.panicked.load(Ordering::Relaxed),
            timed_out: self.timed_out.load(Ordering::Relaxed),
            cancelled: self.cancelled.load(Ordering::Relaxed),
            queue_high_water: self.queue_high_water.load(Ordering::Relaxed),
            stolen: self.stolen.load(Ordering::Relaxed),
        }
    }
}

struct RunningGuard {
    running: Arc<Mutex<HashMap<u64, TaskEntry>>>,
    completed: Arc<Mutex<HashMap<u64, TaskOutput>>>,
    task_id: u64,
    handle: TaskHandle,
    result: Arc<Mutex<Option<TaskOutput>>>,

    finished_normally: bool,
    start: Instant,
    metrics: Arc<ExecutorMetrics>,
}

impl RunningGuard {
    fn new(
        running: Arc<Mutex<HashMap<u64, TaskEntry>>>,
        completed: Arc<Mutex<HashMap<u64, TaskOutput>>>,
        task_id: u64,
        handle: TaskHandle,
        result: Arc<Mutex<Option<TaskOutput>>>,
        start: Instant,
        metrics: Arc<ExecutorMetrics>,
    ) -> Self {
        Self {
            running,
            completed,
            task_id,
            handle,
            result,
            finished_normally: false,
            start,
            metrics,
        }
    }

    fn finish(&mut self) {
        self.finished_normally = true;
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {

        if !self.finished_normally {
            self.metrics.cancelled.fetch_add(1, Ordering::Relaxed);
            let synthetic = TaskOutput {
                handle: self.handle.clone(),
                output: Value::Null,
                success: false,
                error: Some("task cancelled or aborted".into()),
                duration_ms: self.start.elapsed().as_millis() as u64,
                metadata: HashMap::new(),
            };
            let mut slot = self.result.lock();
            if slot.is_none() {
                *slot = Some(synthetic);
            }
        }

        if let Some(output) = self.result.lock().clone() {
            self.completed.lock().insert(self.task_id, output);
        }

        let removed = self.running.lock().remove(&self.task_id);
        if let Some(entry) = removed {
            if let Some(tx) = entry.done_tx.lock().take() {
                let _ = tx.send(());
            }
        }
    }
}

pub struct ParallelExecutor {
    config: ExecutorConfig,

    injector: Arc<Injector<QueuedTask>>,

    stealers: Arc<Vec<Stealer<QueuedTask>>>,

    worker_notifies: Arc<Vec<Arc<Notify>>>,

    next_worker: Arc<AtomicUsize>,

    queued_count: Arc<AtomicUsize>,
    running: Arc<Mutex<HashMap<u64, TaskEntry>>>,

    completed: Arc<Mutex<HashMap<u64, TaskOutput>>>,
    task_counter: Arc<AtomicU64>,

    shutdown: CancellationToken,
    metrics: Arc<ExecutorMetrics>,

    worker_count: usize,

    per_worker_cap: usize,
}

impl Drop for ParallelExecutor {
    fn drop(&mut self) {

        self.shutdown.cancel();
        for n in self.worker_notifies.iter() {
            n.notify_one();
        }
    }
}

impl ParallelExecutor {

    pub fn new(config: ExecutorConfig) -> Self {
        let worker_count = num_cpus::get().min(config.max_concurrent).max(1);

        let mut owned_workers: Vec<DequeWorker<QueuedTask>> = Vec::with_capacity(worker_count);
        let mut stealers: Vec<Stealer<QueuedTask>> = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let w = DequeWorker::<QueuedTask>::new_fifo();
            stealers.push(w.stealer());
            owned_workers.push(w);
        }
        let stealers = Arc::new(stealers);
        let worker_notifies: Vec<Arc<Notify>> = (0..worker_count)
            .map(|_| Arc::new(Notify::new()))
            .collect();
        let worker_notifies = Arc::new(worker_notifies);

        let injector: Arc<Injector<QueuedTask>> = Arc::new(Injector::new());

        let semaphore = Arc::new(Semaphore::new(config.max_concurrent));
        let running: Arc<Mutex<HashMap<u64, TaskEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        let completed: Arc<Mutex<HashMap<u64, TaskOutput>>> = Arc::new(Mutex::new(HashMap::new()));
        let metrics = Arc::new(ExecutorMetrics::default());
        let shutdown = CancellationToken::new();

        let per_worker_cap = (config.queue_capacity / worker_count.max(1)).max(16);

        let executor = Self {
            injector: injector.clone(),
            stealers: stealers.clone(),
            worker_notifies: worker_notifies.clone(),
            next_worker: Arc::new(AtomicUsize::new(0)),
            queued_count: Arc::new(AtomicUsize::new(0)),
            running: running.clone(),
            completed: completed.clone(),
            task_counter: Arc::new(AtomicU64::new(0)),
            shutdown: shutdown.clone(),
            metrics: metrics.clone(),
            worker_count,
            per_worker_cap,
            config,
        };

        for (idx, w) in owned_workers.into_iter().enumerate() {
            let ctx = WorkerLoopContext {
                idx,
                worker: w,
                stealers: stealers.clone(),
                injector: injector.clone(),
                notify: worker_notifies[idx].clone(),
                queued_count: executor.queued_count.clone(),
                semaphore: semaphore.clone(),
                running: running.clone(),
                completed: completed.clone(),
                metrics: metrics.clone(),
                shutdown: shutdown.clone(),
            };
            crate::runtime::spawn_supervised("agent.parallel_executor.worker", worker_loop(ctx));
        }

        executor
    }

    pub fn metrics(&self) -> ExecutorMetricsSnapshot {
        self.metrics.snapshot()
    }

    pub async fn submit<F, Fut>(
        &self,
        task: F,
        _priority: Priority,
        timeout_secs: Option<u64>,
    ) -> Result<TaskHandle, SenError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = TaskOutput> + Send + 'static,
    {
        let task_id = self.task_counter.fetch_add(1, Ordering::Relaxed);
        let handle = TaskHandle::new(task_id);

        {
            let running_len = self.running.lock().len();
            let queued = self.queued_count.load(Ordering::Relaxed);
            if running_len + queued + 1 > self.config.queue_capacity {
                return Err(SenError::Scheduler(SchedulerError::TaskNotFound(format!(
                    "queue at capacity ({} running + {} queued / {})",
                    running_len, queued, self.config.queue_capacity
                ))));
            }
        }

        let result: Arc<Mutex<Option<TaskOutput>>> = Arc::new(Mutex::new(None));
        let done_tx_holder: Arc<Mutex<Option<oneshot::Sender<()>>>> = Arc::new(Mutex::new(None));

        {
            let mut running = self.running.lock();
            running.insert(
                task_id,
                TaskEntry {
                    result: Arc::clone(&result),
                    done_tx: Arc::clone(&done_tx_holder),
                },
            );
            let hw = running.len() as u64;
            let prev = self.metrics.queue_high_water.load(Ordering::Relaxed);
            if hw > prev {
                self.metrics.queue_high_water.store(hw, Ordering::Relaxed);
            }
        }

        let timeout_duration = Duration::from_secs(
            timeout_secs.unwrap_or(self.config.default_timeout_secs.unwrap_or(300)),
        );
        let handle_clone = handle.clone();
        let result_factory = Arc::clone(&result);

        let factory: TaskFutureFactory = Box::new(move || {
            let fut = task();
            Box::pin(fut) as BoxedTaskFuture
        });
        let queued = QueuedTask {
            task_id,
            handle: handle_clone,
            timeout: timeout_duration,
            future_factory: factory,
            result: result_factory,
        };

        let worker_idx =
            self.next_worker.fetch_add(1, Ordering::Relaxed) % self.worker_count.max(1);

        let local_len = self.stealers[worker_idx].len();
        if local_len >= self.per_worker_cap {
            self.injector.push(queued);
        } else {

            self.injector.push(queued);
        }

        self.queued_count.fetch_add(1, Ordering::Relaxed);
        self.metrics.spawned.fetch_add(1, Ordering::Relaxed);

        self.worker_notifies[worker_idx].notify_one();

        Ok(handle)
    }

    pub async fn await_result(
        &self,
        handle: TaskHandle,
        timeout_secs: Option<u64>,
    ) -> Result<TaskOutput, SenError> {
        let (result_arc, done_rx) = {
            let running = self.running.lock();
            match running.get(&handle.id) {
                Some(entry) => {
                    if let Some(output) = entry.result.lock().clone() {
                        return Ok(output);
                    }
                    let (tx, rx) = oneshot::channel::<()>();
                    *entry.done_tx.lock() = Some(tx);
                    (Arc::clone(&entry.result), rx)
                }
                None => {

                    if let Some(output) = self.completed.lock().remove(&handle.id) {
                        return Ok(output);
                    }
                    return Err(SenError::Scheduler(SchedulerError::TaskNotFound(
                        handle.id.to_string(),
                    )));
                }
            }
        };

        let timeout = Duration::from_secs(timeout_secs.unwrap_or(300));
        let task_id = handle.id;
        let completed = Arc::clone(&self.completed);
        let wait = async move {
            let _ = done_rx.await;
            if let Some(output) = result_arc.lock().clone() {

                let _ = completed.lock().remove(&task_id);
                return Ok(output);
            }

            if let Some(output) = completed.lock().remove(&task_id) {
                return Ok(output);
            }
            Err(SenError::Scheduler(SchedulerError::TaskNotFound(
                "result not available".into(),
            )))
        };

        tokio::time::timeout(timeout, wait)
            .await
            .map_err(|_| SenError::Scheduler(SchedulerError::Cancelled))?
    }

    pub async fn aggregate_results(
        &self,
        handles: Vec<TaskHandle>,
        strategy: AggregationStrategy,
        timeout_secs: Option<u64>,
    ) -> Result<AggregatedOutput, SenError> {
        let timeout = Duration::from_secs(timeout_secs.unwrap_or(300));
        let start = Instant::now();
        let complete = self.collect_classified(handles, timeout).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        let filtered = match strategy {
            AggregationStrategy::FirstComplete => first_complete(complete),
            AggregationStrategy::AllComplete => complete,
            AggregationStrategy::BestOfN { n } => best_of_n(complete, n),
            AggregationStrategy::VotingConsensus { threshold } => {
                voting_consensus(complete, threshold, Self::hash_output)
            }
            AggregationStrategy::Cheapest { budget_tokens } => cheapest(complete, budget_tokens),
            AggregationStrategy::Fastest { deadline_ms } => fastest(complete, deadline_ms),
        };

        Ok(AggregatedOutput {
            elapsed_ms,
            ..filtered
        })
    }

    async fn collect_classified(
        &self,
        handles: Vec<TaskHandle>,
        timeout: Duration,
    ) -> AggregatedOutput {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        let start = Instant::now();
        let deadline = start + timeout;

        for handle in handles {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                failed.push(TaskFailure {
                    handle: handle.clone(),
                    reason: FailureReason::Timeout,
                    elapsed_ms: timeout.as_millis() as u64,
                });
                continue;
            }
            match self
                .await_result(handle.clone(), Some(remaining.as_secs().max(1)))
                .await
            {
                Ok(output) if output.success => succeeded.push(output),
                Ok(output) => {
                    let reason = classify_failure(&output);
                    failed.push(TaskFailure {
                        handle: output.handle.clone(),
                        reason,
                        elapsed_ms: output.duration_ms,
                    });
                }
                Err(e) => failed.push(TaskFailure {
                    handle,
                    reason: FailureReason::Internal(e.to_string()),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                }),
            }
        }

        AggregatedOutput {
            succeeded,
            failed,
            elapsed_ms: start.elapsed().as_millis() as u64,
        }
    }

    fn hash_output(output: &Value) -> u64 {
        let mut hasher = DefaultHasher::new();
        serde_json::to_string(output)
            .unwrap_or_default()
            .hash(&mut hasher);
        hasher.finish()
    }

    pub fn stats(&self) -> ExecutorStats {
        let running = self.running.lock();
        let mut queued = self.injector.len();
        for s in self.stealers.iter() {
            queued = queued.saturating_add(s.len());
        }
        ExecutorStats {
            queued,
            running: running.len(),
            max_concurrent: self.config.max_concurrent,
        }
    }
}

struct WorkerLoopContext {
    idx: usize,
    worker: DequeWorker<QueuedTask>,
    stealers: Arc<Vec<Stealer<QueuedTask>>>,
    injector: Arc<Injector<QueuedTask>>,
    notify: Arc<Notify>,
    queued_count: Arc<AtomicUsize>,
    semaphore: Arc<Semaphore>,
    running: Arc<Mutex<HashMap<u64, TaskEntry>>>,
    completed: Arc<Mutex<HashMap<u64, TaskOutput>>>,
    metrics: Arc<ExecutorMetrics>,
    shutdown: CancellationToken,
}

async fn worker_loop(ctx: WorkerLoopContext) {
    let WorkerLoopContext {
        idx,
        worker,
        stealers,
        injector,
        notify,
        queued_count,
        semaphore,
        running,
        completed,
        metrics,
        shutdown,
    } = ctx;

    loop {
        if shutdown.is_cancelled() {
            return;
        }

        let maybe_task = find_task(idx, &worker, &stealers, &injector, &metrics);

        match maybe_task {
            Some(task) => {
                queued_count.fetch_sub(1, Ordering::Relaxed);

                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => return,
                };

                let busy_start = Instant::now();
                let timeout = task.timeout;
                let task_id = task.task_id;
                let handle = task.handle.clone();
                let result = task.result.clone();
                let factory = task.future_factory;
                let running_clone = running.clone();
                let completed_clone = completed.clone();
                let metrics_clone = metrics.clone();
                let worker_idx = idx;

                crate::runtime::spawn_supervised("agent.parallel_executor.task", async move {
                    let start = busy_start;
                    let mut guard = RunningGuard::new(
                        running_clone,
                        completed_clone,
                        task_id,
                        handle.clone(),
                        result.clone(),
                        start,
                        metrics_clone.clone(),
                    );

                    let user_future = factory();
                    let fut = std::panic::AssertUnwindSafe(user_future);
                    let outcome = tokio::time::timeout(timeout, fut.catch_unwind()).await;

                    let task_output = match outcome {
                        Ok(Ok(output)) => {
                            metrics_clone.completed.fetch_add(1, Ordering::Relaxed);
                            output
                        }
                        Ok(Err(panic_payload)) => {
                            metrics_clone.panicked.fetch_add(1, Ordering::Relaxed);
                            let msg = extract_panic_message(&panic_payload);
                            TaskOutput {
                                handle: handle.clone(),
                                output: Value::Null,
                                success: false,
                                error: Some(format!("panic: {msg}")),
                                duration_ms: start.elapsed().as_millis() as u64,
                                metadata: HashMap::new(),
                            }
                        }
                        Err(_) => {
                            metrics_clone.timed_out.fetch_add(1, Ordering::Relaxed);
                            TaskOutput {
                                handle: handle.clone(),
                                output: Value::Null,
                                success: false,
                                error: Some("task timed out".into()),
                                duration_ms: start.elapsed().as_millis() as u64,
                                metadata: HashMap::new(),
                            }
                        }
                    };

                    drop(permit);

                    {
                        let mut slot = result.lock();
                        *slot = Some(task_output);
                    }

                    scheduler_metrics::add_worker_busy_nanos(
                        worker_idx,
                        start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
                    );

                    guard.finish();
                    drop(guard);
                });
            }
            None => {

                tokio::select! {
                    _ = notify.notified() => {}
                    _ = shutdown.cancelled() => return,
                }
            }
        }
    }
}

fn find_task(
    self_idx: usize,
    worker: &DequeWorker<QueuedTask>,
    stealers: &[Stealer<QueuedTask>],
    injector: &Injector<QueuedTask>,
    metrics: &ExecutorMetrics,
) -> Option<QueuedTask> {
    if let Some(t) = worker.pop() {
        return Some(t);
    }

    let n = stealers.len();
    if n > 1 {
        for off in 1..n {
            let peer_idx = (self_idx + off) % n;
            let steal = stealers[peer_idx].steal_batch_and_pop(worker);
            if let Steal::Success(task) = steal {
                metrics.stolen.fetch_add(1, Ordering::Relaxed);
                scheduler_metrics::incr_steal_events(1);
                return Some(task);
            }
        }
    }

    loop {
        match injector.steal_batch_and_pop(worker) {
            Steal::Empty => return None,
            Steal::Retry => continue,
            Steal::Success(task) => {
                metrics.stolen.fetch_add(1, Ordering::Relaxed);
                scheduler_metrics::incr_steal_events(1);
                return Some(task);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecutorStats {
    pub queued: usize,
    pub running: usize,
    pub max_concurrent: usize,
}

fn first_complete(mut agg: AggregatedOutput) -> AggregatedOutput {
    agg.succeeded.truncate(1);
    agg
}

fn best_of_n(mut agg: AggregatedOutput, n: usize) -> AggregatedOutput {

    agg.succeeded
        .sort_by(|a, b| a.duration_ms.cmp(&b.duration_ms));
    agg.succeeded.truncate(n);
    agg
}

fn voting_consensus(
    agg: AggregatedOutput,
    threshold: f64,
    hash_fn: fn(&Value) -> u64,
) -> AggregatedOutput {

    let total_all = (agg.succeeded.len() + agg.failed.len()) as f64;
    if total_all == 0.0 {
        return AggregatedOutput {
            succeeded: Vec::new(),
            failed: agg.failed,
            elapsed_ms: agg.elapsed_ms,
        };
    }
    let mut buckets: HashMap<u64, Vec<TaskOutput>> = HashMap::new();
    for out in &agg.succeeded {
        let h = hash_fn(&out.output);
        buckets.entry(h).or_default().push(out.clone());
    }
    let mut consensus = Vec::new();
    for (_, bucket) in buckets {
        if (bucket.len() as f64) / total_all >= threshold {
            consensus.extend(bucket);
        }
    }
    AggregatedOutput {
        succeeded: consensus,
        failed: agg.failed,
        elapsed_ms: agg.elapsed_ms,
    }
}

fn cheapest(agg: AggregatedOutput, budget_tokens: u64) -> AggregatedOutput {
    let mut under_budget: Vec<(u64, TaskOutput)> = agg
        .succeeded
        .iter()
        .filter_map(|r| {
            let cost = r
                .metadata
                .get("cost_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX);
            if cost <= budget_tokens {
                Some((cost, r.clone()))
            } else {
                None
            }
        })
        .collect();
    under_budget.sort_by_key(|(c, _)| *c);
    AggregatedOutput {
        succeeded: under_budget
            .into_iter()
            .next()
            .map(|(_, r)| vec![r])
            .unwrap_or_default(),
        failed: agg.failed,
        elapsed_ms: agg.elapsed_ms,
    }
}

fn fastest(agg: AggregatedOutput, deadline_ms: u64) -> AggregatedOutput {
    let cap = deadline_ms as u64;
    let chosen = agg.succeeded.iter().find(|r| r.duration_ms <= cap).cloned();
    AggregatedOutput {
        succeeded: chosen.map(|r| vec![r]).unwrap_or_default(),
        failed: agg.failed,
        elapsed_ms: agg.elapsed_ms,
    }
}

fn classify_failure(output: &TaskOutput) -> FailureReason {
    match output.error.as_deref() {
        Some(s) if s.starts_with("panic:") => FailureReason::Panic(s.to_string()),
        Some(s) if s.contains("timed out") => FailureReason::Timeout,
        Some(s) if s.contains("cancelled") || s.contains("aborted") => FailureReason::Cancelled,
        Some(s) => FailureReason::Application(s.to_string()),
        None => FailureReason::Application("(no error message)".into()),
    }
}

fn extract_panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<opaque panic payload>".to_string()
}
