// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::agent::task_orchestrator::queue::{TaskId, TaskPriority, TaskStatus};
use crate::memory::blackboard::BlackboardHandle;
use crate::observability::scheduler_metrics;

pub const SCHEDULER_EVENT_CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
pub struct SchedulableTask {
    pub id: TaskId,
    pub description: String,
    pub prompt: String,
    pub required_capability: String,
    pub priority: TaskPriority,
    pub depends_on: Vec<TaskId>,
}

impl SchedulableTask {
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            prompt: prompt.into(),
            required_capability: "general".into(),
            priority: TaskPriority::Normal,
            depends_on: Vec::new(),
        }
    }

    pub fn with_dependency(mut self, dep: impl Into<String>) -> Self {
        self.depends_on.push(dep.into());
        self
    }

    pub fn with_priority(mut self, p: TaskPriority) -> Self {
        self.priority = p;
        self
    }
}

#[derive(Debug, Clone)]
pub struct TaskOutcome {
    pub task_id: TaskId,
    pub success: bool,
    pub result: String,
    pub assigned_agent: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    TaskReady { id: TaskId, priority: TaskPriority },
    TaskCompleted { id: TaskId },
    TaskFailed { id: TaskId, error: String },
    TaskCancelled { id: TaskId },
    GraphCompleted,
}

#[derive(Debug)]
struct TaskNode {
    task: SchedulableTask,
    status: TaskStatus,
    remaining_deps: HashSet<TaskId>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ReadyEntry {
    priority: TaskPriority,
    seq: u64,
    task_id: TaskId,
}

impl Ord for ReadyEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for ReadyEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn priority_label(p: TaskPriority) -> scheduler_metrics::TaskPriorityLabel {
    match p {
        TaskPriority::Critical => scheduler_metrics::TaskPriorityLabel::Critical,
        TaskPriority::High => scheduler_metrics::TaskPriorityLabel::High,
        TaskPriority::Normal => scheduler_metrics::TaskPriorityLabel::Normal,
        TaskPriority::Low => scheduler_metrics::TaskPriorityLabel::Low,
        TaskPriority::Background => scheduler_metrics::TaskPriorityLabel::Background,
    }
}

pub struct TaskScheduler {
    nodes: HashMap<TaskId, TaskNode>,
    dependents: HashMap<TaskId, Vec<TaskId>>,
    ready_queue: BinaryHeap<ReadyEntry>,
    max_parallel: usize,
    cancellation: CancellationToken,
    outcomes: Arc<Mutex<Vec<TaskOutcome>>>,
    events: broadcast::Sender<SchedulerEvent>,
    seq: AtomicU64,
    graph_completed_sent: bool,
}

impl TaskScheduler {
    pub fn new(max_parallel: usize) -> Self {
        let (tx, _) = broadcast::channel(SCHEDULER_EVENT_CAPACITY);
        Self {
            nodes: HashMap::new(),
            dependents: HashMap::new(),
            ready_queue: BinaryHeap::new(),
            max_parallel: max_parallel.max(1),
            cancellation: CancellationToken::new(),
            outcomes: Arc::new(Mutex::new(Vec::new())),
            events: tx,
            seq: AtomicU64::new(0),
            graph_completed_sent: false,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SchedulerEvent> {
        self.events.subscribe()
    }

    pub fn ready_snapshot(&self) -> Vec<TaskId> {
        let mut entries: Vec<ReadyEntry> = self.ready_queue.iter().cloned().collect();
        entries.sort_by(|a, b| b.cmp(a));
        entries.into_iter().map(|e| e.task_id).collect()
    }

    fn push_ready(&mut self, task_id: TaskId, priority: TaskPriority) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        self.ready_queue.push(ReadyEntry {
            priority,
            seq,
            task_id: task_id.clone(),
        });
        scheduler_metrics::set_ready_queue_depth(self.ready_queue.len() as i64);
        let _ = self
            .events
            .send(SchedulerEvent::TaskReady { id: task_id, priority });
    }

    fn maybe_emit_graph_completed(&mut self) {
        if !self.graph_completed_sent && self.is_finished() {
            self.graph_completed_sent = true;
            let _ = self.events.send(SchedulerEvent::GraphCompleted);
        }
    }

    pub fn add_tasks(&mut self, tasks: Vec<SchedulableTask>) -> Result<(), String> {
        let existing_ids: HashSet<&str> = self.nodes.keys().map(|s| s.as_str()).collect();
        let mut new_ids_seen: HashSet<&str> = HashSet::new();
        for t in &tasks {
            if existing_ids.contains(t.id.as_str()) {
                return Err(format!(
                    "Task '{}' already scheduled; use unique ids when appending",
                    t.id
                ));
            }
            if !new_ids_seen.insert(t.id.as_str()) {
                return Err(format!("Duplicate task id '{}' inside add_tasks batch", t.id));
            }
        }
        let new_ids: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        let all_ids: HashSet<&str> = existing_ids.union(&new_ids).copied().collect();

        for task in &tasks {
            for dep in &task.depends_on {
                if !all_ids.contains(dep.as_str()) {
                    return Err(format!(
                        "Task '{}' depends on unknown task '{}'",
                        task.id, dep
                    ));
                }
            }
        }

        if has_cycle_combined(self.nodes.keys().cloned().collect(), &tasks) {
            return Err("Dependency graph contains a cycle".into());
        }

        let batch_len = tasks.len();
        for task in tasks {
            let remaining_deps: HashSet<TaskId> = task
                .depends_on
                .iter()
                .filter(|d| {

                    !matches!(
                        self.nodes.get(d.as_str()).map(|n| &n.status),
                        Some(TaskStatus::Completed)
                    )
                })
                .cloned()
                .collect();
            let is_ready = remaining_deps.is_empty();
            let id = task.id.clone();
            let priority = task.priority;

            for dep in &task.depends_on {
                self.dependents
                    .entry(dep.clone())
                    .or_default()
                    .push(id.clone());
            }

            self.nodes.insert(
                id.clone(),
                TaskNode {
                    task,
                    status: TaskStatus::Queued,
                    remaining_deps,
                },
            );

            if is_ready {
                self.push_ready(id, priority);
            }
        }

        if batch_len > 0 {
            scheduler_metrics::incr_dag_nodes(batch_len as u64);
        }
        self.graph_completed_sent = false;
        self.maybe_emit_graph_completed();
        Ok(())
    }

    pub fn add_task(&mut self, task: SchedulableTask) -> Result<(), String> {
        self.add_tasks(vec![task])
    }

    pub fn claim_next(&mut self) -> Option<SchedulableTask> {
        while let Some(entry) = self.ready_queue.pop() {
            scheduler_metrics::set_ready_queue_depth(self.ready_queue.len() as i64);
            if let Some(task) = self.try_claim_inner(&entry.task_id) {
                return Some(task);
            }
        }
        None
    }

    pub fn try_claim(&mut self, task_id: &str) -> Option<SchedulableTask> {
        let claimed = self.try_claim_inner(task_id);
        if claimed.is_none() {
            scheduler_metrics::incr_try_claim_miss();
        } else {

            self.remove_from_heap(task_id);
        }
        claimed
    }

    fn try_claim_inner(&mut self, task_id: &str) -> Option<SchedulableTask> {
        let node = self.nodes.get_mut(task_id)?;
        if node.status != TaskStatus::Queued {
            return None;
        }
        node.status = TaskStatus::Running;
        scheduler_metrics::incr_task_started(priority_label(node.task.priority));
        Some(node.task.clone())
    }

    fn remove_from_heap(&mut self, task_id: &str) {

        let drained: Vec<ReadyEntry> = self
            .ready_queue
            .drain()
            .filter(|e| e.task_id != task_id)
            .collect();
        self.ready_queue = drained.into_iter().collect();
        scheduler_metrics::set_ready_queue_depth(self.ready_queue.len() as i64);
    }

    pub fn complete(&mut self, task_id: &str, result: String) {
        self.complete_with_agent(task_id, result, None)
    }

    pub fn complete_with_agent(
        &mut self,
        task_id: &str,
        result: String,
        assigned_agent: Option<String>,
    ) {
        if let Some(node) = self.nodes.get_mut(task_id) {
            node.status = TaskStatus::Completed;
        }

        self.outcomes.lock().push(TaskOutcome {
            task_id: task_id.to_string(),
            success: true,
            result,
            assigned_agent,
        });

        let mut ready_now: Vec<(TaskId, TaskPriority)> = Vec::new();
        if let Some(deps) = self.dependents.get(task_id) {
            for dep_id in deps.clone() {
                if let Some(dep_node) = self.nodes.get_mut(&dep_id) {
                    dep_node.remaining_deps.remove(task_id);
                    if dep_node.remaining_deps.is_empty() && dep_node.status == TaskStatus::Queued {
                        ready_now.push((dep_id, dep_node.task.priority));
                    }
                }
            }
        }
        for (id, prio) in ready_now {
            self.push_ready(id, prio);
        }

        let _ = self.events.send(SchedulerEvent::TaskCompleted {
            id: task_id.to_string(),
        });
        self.maybe_emit_graph_completed();
    }

    pub fn fail(&mut self, task_id: &str, error: String) {
        self.fail_with_agent(task_id, error, None)
    }

    pub fn fail_with_agent(
        &mut self,
        task_id: &str,
        error: String,
        assigned_agent: Option<String>,
    ) {
        if let Some(node) = self.nodes.get_mut(task_id) {
            node.status = TaskStatus::Failed;
        }

        self.outcomes.lock().push(TaskOutcome {
            task_id: task_id.to_string(),
            success: false,
            result: error.clone(),
            assigned_agent,
        });

        let _ = self.events.send(SchedulerEvent::TaskFailed {
            id: task_id.to_string(),
            error,
        });

        self.cancel_dependents(task_id);
        self.maybe_emit_graph_completed();
    }

    fn cancel_dependents(&mut self, task_id: &str) {
        let mut to_cancel = VecDeque::new();
        if let Some(deps) = self.dependents.get(task_id) {
            to_cancel.extend(deps.iter().cloned());
        }
        while let Some(id) = to_cancel.pop_front() {
            if let Some(node) = self.nodes.get_mut(&id) {
                if node.status == TaskStatus::Queued {
                    node.status = TaskStatus::Cancelled;
                    self.remove_from_heap(&id);
                    let _ = self
                        .events
                        .send(SchedulerEvent::TaskCancelled { id: id.clone() });
                    if let Some(further) = self.dependents.get(&id) {
                        to_cancel.extend(further.iter().cloned());
                    }
                }
            }
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn cancel_all(&self) {
        self.cancellation.cancel();
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    pub fn ready_count(&self) -> usize {
        self.ready_queue.len()
    }

    pub fn running_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.status == TaskStatus::Running)
            .count()
    }

    pub fn is_finished(&self) -> bool {
        !self.nodes.is_empty()
            && self.nodes.values().all(|n| {
                matches!(
                    n.status,
                    TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                )
            })
    }

    pub fn outcomes(&self) -> Vec<TaskOutcome> {
        self.outcomes.lock().clone()
    }

    pub fn flush_to_blackboard(&self, bb: &BlackboardHandle) {
        for outcome in self.outcomes.lock().iter() {
            if outcome.success {
                bb.inner().write(
                    &outcome.task_id,
                    serde_json::json!({ "result": &outcome.result }),
                    "scheduler",
                    "task_results",
                );
            }
        }
    }
}

fn has_cycle_combined(existing: Vec<TaskId>, batch: &[SchedulableTask]) -> bool {
    let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
    let mut adj: HashMap<TaskId, Vec<TaskId>> = HashMap::new();

    for id in existing {
        in_degree.entry(id).or_insert(0);
    }
    for t in batch {
        in_degree.entry(t.id.clone()).or_insert(0);
    }
    for t in batch {
        for dep in &t.depends_on {
            adj.entry(dep.clone()).or_default().push(t.id.clone());
            *in_degree.entry(t.id.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<TaskId> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    let total = in_degree.len();
    let mut visited = 0usize;
    while let Some(id) = queue.pop_front() {
        visited += 1;
        if let Some(neighbors) = adj.get(&id) {
            for n in neighbors.clone() {
                if let Some(d) = in_degree.get_mut(&n) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        queue.push_back(n);
                    }
                }
            }
        }
    }

    visited != total
}
