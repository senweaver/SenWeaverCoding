// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;
use std::time::Instant;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::json;
use tracing::warn;

use super::types::{SopRun, SopRunStatus, SopStepStatus};
use crate::memory::traits::{Memory, MemoryCategory};

const MAX_RECENT_RUNS: usize = 1000;

const PENDING_EVICT_SECS: u64 = 3600;

#[derive(Debug, Default, Clone)]
struct MetricCounters {
    runs_completed: u64,
    runs_failed: u64,
    runs_cancelled: u64,
    steps_executed: u64,
    steps_defined: u64,
    steps_failed: u64,
    steps_skipped: u64,
    human_approvals: u64,
    timeout_auto_approvals: u64,
}

#[derive(Debug, Clone)]
struct RunSnapshot {
    completed_at: DateTime<Utc>,
    terminal_status: SopRunStatus,
    steps_executed: u64,
    steps_defined: u64,
    steps_failed: u64,
    steps_skipped: u64,
    human_approval_count: u64,
    timeout_approval_count: u64,
}

#[derive(Debug, Default)]
struct SopCounters {
    counters: MetricCounters,
    recent_runs: VecDeque<RunSnapshot>,
}

#[derive(Debug, Default)]
struct CollectorState {
    global: SopCounters,
    per_sop: HashMap<String, SopCounters>,

    pending_approvals: HashMap<String, (Instant, u64)>,

    pending_timeout_approvals: HashMap<String, (Instant, u64)>,
}

pub struct SopMetricsCollector {
    inner: RwLock<CollectorState>,
}

impl SopMetricsCollector {

    pub fn new() -> Self {
        Self {
            inner: RwLock::new(CollectorState::default()),
        }
    }

    pub fn record_run_complete(&self, run: &SopRun) {
        let Ok(mut state) = self.inner.write() else {
            warn!("SOP metrics collector lock poisoned in record_run_complete");
            return;
        };

        let now = Instant::now();
        state
            .pending_approvals
            .retain(|_, (ts, _)| now.duration_since(*ts).as_secs() < PENDING_EVICT_SECS);
        state
            .pending_timeout_approvals
            .retain(|_, (ts, _)| now.duration_since(*ts).as_secs() < PENDING_EVICT_SECS);

        let human_count = state
            .pending_approvals
            .remove(&run.run_id)
            .map(|(_, c)| c)
            .unwrap_or(0);
        let timeout_count = state
            .pending_timeout_approvals
            .remove(&run.run_id)
            .map(|(_, c)| c)
            .unwrap_or(0);

        let snapshot = build_snapshot(run, human_count, timeout_count);
        apply_run(&mut state.global, &snapshot);
        let counters = state.per_sop.entry(run.sop_name.clone()).or_default();
        apply_run(counters, &snapshot);
    }

    pub fn record_approval(&self, sop_name: &str, run_id: &str) {
        let Ok(mut state) = self.inner.write() else {
            warn!("SOP metrics collector lock poisoned in record_approval");
            return;
        };
        state.global.counters.human_approvals += 1;
        state
            .per_sop
            .entry(sop_name.to_string())
            .or_default()
            .counters
            .human_approvals += 1;
        let entry = state
            .pending_approvals
            .entry(run_id.to_string())
            .or_insert((Instant::now(), 0));
        entry.0 = Instant::now();
        entry.1 += 1;
    }

    pub fn record_timeout_auto_approve(&self, sop_name: &str, run_id: &str) {
        let Ok(mut state) = self.inner.write() else {
            warn!("SOP metrics collector lock poisoned in record_timeout_auto_approve");
            return;
        };
        state.global.counters.timeout_auto_approvals += 1;
        state
            .per_sop
            .entry(sop_name.to_string())
            .or_default()
            .counters
            .timeout_auto_approvals += 1;
        let entry = state
            .pending_timeout_approvals
            .entry(run_id.to_string())
            .or_insert((Instant::now(), 0));
        entry.0 = Instant::now();
        entry.1 += 1;
    }

    pub async fn rebuild_from_memory(memory: &dyn Memory) -> anyhow::Result<Self> {
        let category = MemoryCategory::Custom("sop".into());
        let entries = memory.list(Some(&category), None).await?;

        let mut runs: HashMap<String, SopRun> = HashMap::new();
        let mut approval_counts: HashMap<String, u64> = HashMap::new();
        let mut timeout_counts: HashMap<String, u64> = HashMap::new();

        let mut approval_sop_names: HashMap<String, String> = HashMap::new();

        for entry in &entries {
            if entry.key.starts_with("sop_run_") {
                if let Ok(run) = serde_json::from_str::<SopRun>(&entry.content) {
                    if matches!(
                        run.status,
                        SopRunStatus::Completed | SopRunStatus::Failed | SopRunStatus::Cancelled
                    ) {
                        runs.insert(run.run_id.clone(), run);
                    }
                }
            } else if entry.key.starts_with("sop_approval_") {
                if let Ok(run) = serde_json::from_str::<SopRun>(&entry.content) {
                    *approval_counts.entry(run.run_id.clone()).or_default() += 1;
                    approval_sop_names
                        .entry(run.run_id.clone())
                        .or_insert(run.sop_name);
                }
            } else if entry.key.starts_with("sop_timeout_approve_") {
                if let Ok(run) = serde_json::from_str::<SopRun>(&entry.content) {
                    *timeout_counts.entry(run.run_id.clone()).or_default() += 1;
                    approval_sop_names
                        .entry(run.run_id.clone())
                        .or_insert(run.sop_name);
                }
            }
        }

        let mut state = CollectorState::default();
        for (run_id, run) in &runs {
            let human_count = approval_counts.get(run_id).copied().unwrap_or(0);
            let timeout_count = timeout_counts.get(run_id).copied().unwrap_or(0);
            let snapshot = build_snapshot(run, human_count, timeout_count);
            apply_run(&mut state.global, &snapshot);
            let counters = state.per_sop.entry(run.sop_name.clone()).or_default();
            apply_run(counters, &snapshot);
        }

        for (run_id, count) in &approval_counts {
            state.global.counters.human_approvals += count;
            if let Some(sop_name) = approval_sop_names.get(run_id) {
                state
                    .per_sop
                    .entry(sop_name.clone())
                    .or_default()
                    .counters
                    .human_approvals += count;
            }
        }
        for (run_id, count) in &timeout_counts {
            state.global.counters.timeout_auto_approvals += count;
            if let Some(sop_name) = approval_sop_names.get(run_id) {
                state
                    .per_sop
                    .entry(sop_name.clone())
                    .or_default()
                    .counters
                    .timeout_auto_approvals += count;
            }
        }

        for (run_id, count) in &approval_counts {
            if !runs.contains_key(run_id) {
                state
                    .pending_approvals
                    .insert(run_id.clone(), (Instant::now(), *count));
            }
        }
        for (run_id, count) in &timeout_counts {
            if !runs.contains_key(run_id) {
                state
                    .pending_timeout_approvals
                    .insert(run_id.clone(), (Instant::now(), *count));
            }
        }

        Ok(Self {
            inner: RwLock::new(state),
        })
    }

    pub fn get_metric_value(&self, name: &str) -> Option<serde_json::Value> {
        let Ok(state) = self.inner.read() else {
            return None;
        };

        let rest = name.strip_prefix("sop.")?;

        if let Some(val) = resolve_metric(&state.global, rest) {
            return Some(val);
        }

        let mut best_key: Option<&str> = None;
        let mut best_len = 0;
        for key in state.per_sop.keys() {
            if rest.starts_with(key.as_str()) {
                let next_char_idx = key.len();

                if rest.len() > next_char_idx
                    && rest.as_bytes()[next_char_idx] == b'.'
                    && key.len() > best_len
                {
                    best_key = Some(key.as_str());
                    best_len = key.len();
                }
            }
        }

        if let Some(sop_key) = best_key {
            let suffix = &rest[sop_key.len() + 1..];
            if let Some(counters) = state.per_sop.get(sop_key) {
                return resolve_metric(counters, suffix);
            }
        }

        None
    }

    pub fn get_metric_value_windowed(
        &self,
        name: &str,
        window: &std::time::Duration,
    ) -> Option<serde_json::Value> {
        let state = self.inner.read().ok()?;
        let rest = name.strip_prefix("sop.")?;

        let (counters, metric_name) = if let Some(dot) = rest.find('.') {

            let mut best_key: Option<&str> = None;
            let mut best_len = 0;
            for key in state.per_sop.keys() {
                if rest.starts_with(key.as_str()) {
                    let next_char_idx = key.len();
                    if rest.len() > next_char_idx
                        && rest.as_bytes()[next_char_idx] == b'.'
                        && key.len() > best_len
                    {
                        best_key = Some(key.as_str());
                        best_len = key.len();
                    }
                }
            }
            if let Some(sop_key) = best_key {
                let suffix = &rest[sop_key.len() + 1..];
                match state.per_sop.get(sop_key) {
                    Some(c) => (c, suffix),
                    None => return None,
                }
            } else {

                let _ = dot;
                (&state.global, rest)
            }
        } else {

            (&state.global, rest)
        };

        let cutoff = Utc::now() - chrono::Duration::from_std(*window).ok()?;
        let wc = aggregate_windowed(&counters.recent_runs, cutoff);
        resolve_from_counters(&wc, metric_name)
    }

    pub fn snapshot(&self) -> serde_json::Value {
        let Ok(state) = self.inner.read() else {
            return json!({"error": "lock poisoned"});
        };

        let per_sop: serde_json::Map<String, serde_json::Value> = state
            .per_sop
            .iter()
            .map(|(name, c)| (name.clone(), counters_to_json(c)))
            .collect();

        json!({
            "global": counters_to_json(&state.global),
            "per_sop": per_sop,
            "pending_approvals": state.pending_approvals.len(),
            "pending_timeout_approvals": state.pending_timeout_approvals.len(),
        })
    }
}

impl Default for SopMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

fn build_snapshot(run: &SopRun, human_count: u64, timeout_count: u64) -> RunSnapshot {
    let completed_at = run
        .completed_at
        .as_deref()
        .and_then(parse_completed_at)
        .unwrap_or_else(Utc::now);

    let steps_executed = run.step_results.len() as u64;
    let steps_failed = run
        .step_results
        .iter()
        .filter(|s| s.status == SopStepStatus::Failed)
        .count() as u64;
    let steps_skipped = run
        .step_results
        .iter()
        .filter(|s| s.status == SopStepStatus::Skipped)
        .count() as u64;

    RunSnapshot {
        completed_at,
        terminal_status: run.status,
        steps_executed,
        steps_defined: u64::from(run.total_steps),
        steps_failed,
        steps_skipped,
        human_approval_count: human_count,
        timeout_approval_count: timeout_count,
    }
}

fn apply_run(sop: &mut SopCounters, snap: &RunSnapshot) {
    let c = &mut sop.counters;
    match snap.terminal_status {
        SopRunStatus::Completed => c.runs_completed += 1,
        SopRunStatus::Failed => c.runs_failed += 1,
        SopRunStatus::Cancelled => c.runs_cancelled += 1,
        _ => {}
    }
    c.steps_executed += snap.steps_executed;
    c.steps_defined += snap.steps_defined;
    c.steps_failed += snap.steps_failed;
    c.steps_skipped += snap.steps_skipped;

    sop.recent_runs.push_back(snap.clone());
    if sop.recent_runs.len() > MAX_RECENT_RUNS {
        sop.recent_runs.pop_front();
    }
}

fn parse_completed_at(ts: &str) -> Option<DateTime<Utc>> {

    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&Utc));
    }

    if let Ok(n) = NaiveDateTime::parse_from_str(ts.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S") {
        return Some(n.and_utc());
    }

    warn!("SOP metrics: could not parse completed_at timestamp: {ts}");
    None
}

fn aggregate_windowed(
    recent_runs: &VecDeque<RunSnapshot>,
    cutoff: DateTime<Utc>,
) -> MetricCounters {
    let mut wc = MetricCounters::default();
    for snap in recent_runs {
        if snap.completed_at >= cutoff {
            match snap.terminal_status {
                SopRunStatus::Completed => wc.runs_completed += 1,
                SopRunStatus::Failed => wc.runs_failed += 1,
                SopRunStatus::Cancelled => wc.runs_cancelled += 1,
                _ => {}
            }
            wc.steps_executed += snap.steps_executed;
            wc.steps_defined += snap.steps_defined;
            wc.steps_failed += snap.steps_failed;
            wc.steps_skipped += snap.steps_skipped;
            wc.human_approvals += snap.human_approval_count;
            wc.timeout_auto_approvals += snap.timeout_approval_count;
        }
    }
    wc
}

fn resolve_metric(sop: &SopCounters, suffix: &str) -> Option<serde_json::Value> {

    let (base, window_days) = if let Some(base) = suffix.strip_suffix("_7d") {
        (base, Some(7i64))
    } else if let Some(base) = suffix.strip_suffix("_30d") {
        (base, Some(30i64))
    } else if let Some(base) = suffix.strip_suffix("_90d") {
        (base, Some(90i64))
    } else {
        (suffix, None)
    };

    if let Some(days) = window_days {
        let cutoff = Utc::now() - chrono::Duration::days(days);
        let wc = aggregate_windowed(&sop.recent_runs, cutoff);
        resolve_from_counters(&wc, base)
    } else {
        resolve_from_counters(&sop.counters, base)
    }
}

fn resolve_from_counters(c: &MetricCounters, metric: &str) -> Option<serde_json::Value> {
    match metric {
        "runs_completed" => Some(json!(c.runs_completed)),
        "runs_failed" => Some(json!(c.runs_failed)),
        "runs_cancelled" => Some(json!(c.runs_cancelled)),
        "deviation_rate" => {
            if c.steps_executed == 0 {
                Some(json!(0.0))
            } else {
                Some(json!(
                    (c.steps_failed + c.steps_skipped) as f64 / c.steps_executed as f64
                ))
            }
        }
        "protocol_adherence_rate" => {
            if c.steps_defined == 0 {
                Some(json!(0.0))
            } else {
                let good = c
                    .steps_executed
                    .saturating_sub(c.steps_failed)
                    .saturating_sub(c.steps_skipped);
                Some(json!(good as f64 / c.steps_defined as f64))
            }
        }
        "human_intervention_count" => Some(json!(c.human_approvals)),
        "human_intervention_rate" => Some(json!(
            c.human_approvals as f64 / c.runs_completed.max(1) as f64
        )),
        "timeout_auto_approvals" => Some(json!(c.timeout_auto_approvals)),
        "timeout_approval_rate" => Some(json!(
            c.timeout_auto_approvals as f64 / c.runs_completed.max(1) as f64
        )),
        "completion_rate" => {
            let total = c.runs_completed + c.runs_failed + c.runs_cancelled;
            Some(json!(c.runs_completed as f64 / total.max(1) as f64))
        }
        _ => None,
    }
}

fn counters_to_json(sop: &SopCounters) -> serde_json::Value {
    let c = &sop.counters;
    json!({
        "runs_completed": c.runs_completed,
        "runs_failed": c.runs_failed,
        "runs_cancelled": c.runs_cancelled,
        "steps_executed": c.steps_executed,
        "steps_defined": c.steps_defined,
        "steps_failed": c.steps_failed,
        "steps_skipped": c.steps_skipped,
        "human_approvals": c.human_approvals,
        "timeout_auto_approvals": c.timeout_auto_approvals,
        "recent_runs_depth": sop.recent_runs.len(),
    })
}

