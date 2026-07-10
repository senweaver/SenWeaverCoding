// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use tracing::{info, warn};

use super::condition::evaluate_condition;
use super::load_sops;
use super::types::{
    DeterministicRunState, DeterministicSavings, Sop, SopEvent, SopExecutionMode, SopPriority,
    SopRun, SopRunAction, SopRunStatus, SopStep, SopStepKind, SopStepResult, SopStepStatus,
    SopTrigger, SopTriggerSource,
};
use crate::config::SopConfig;

pub struct SopEngine {
    sops: Vec<Sop>,
    active_runs: HashMap<String, SopRun>,

    finished_runs: Vec<SopRun>,
    config: SopConfig,
    run_counter: u64,

    deterministic_savings: DeterministicSavings,
}

static GLOBAL_SOP_ENGINE: std::sync::OnceLock<
    std::sync::Arc<parking_lot::Mutex<SopEngine>>,
> = std::sync::OnceLock::new();

pub fn global_sop_engine(
    config: &SopConfig,
) -> std::sync::Arc<parking_lot::Mutex<SopEngine>> {
    GLOBAL_SOP_ENGINE
        .get_or_init(|| {
            std::sync::Arc::new(parking_lot::Mutex::new(SopEngine::new(config.clone())))
        })
        .clone()
}

impl SopEngine {

    pub fn new(config: SopConfig) -> Self {
        Self {
            sops: Vec::new(),
            active_runs: HashMap::new(),
            finished_runs: Vec::new(),
            config,
            run_counter: 0,
            deterministic_savings: DeterministicSavings::default(),
        }
    }

    pub fn reload(&mut self, workspace_dir: &Path) {
        self.sops = load_sops(
            workspace_dir,
            self.config.sops_dir.as_deref(),
            super::parse_execution_mode(&self.config.default_execution_mode),
        );
        info!("SOP engine loaded {} SOPs", self.sops.len());
    }

    pub fn sops(&self) -> &[Sop] {
        &self.sops
    }

    pub fn active_runs(&self) -> &HashMap<String, SopRun> {
        &self.active_runs
    }

    pub fn get_run(&self, run_id: &str) -> Option<&SopRun> {
        self.active_runs
            .get(run_id)
            .or_else(|| self.finished_runs.iter().find(|r| r.run_id == run_id))
    }

    pub fn get_sop(&self, name: &str) -> Option<&Sop> {
        self.sops.iter().find(|s| s.name == name)
    }

    pub fn match_trigger(&self, event: &SopEvent) -> Vec<&Sop> {
        self.sops
            .iter()
            .filter(|sop| sop.triggers.iter().any(|t| trigger_matches(t, event)))
            .collect()
    }

    pub fn can_start(&self, sop_name: &str) -> bool {
        let sop = match self.get_sop(sop_name) {
            Some(s) => s,
            None => return false,
        };

        let active_for_sop = self
            .active_runs
            .values()
            .filter(|r| r.sop_name == sop_name)
            .count();
        if active_for_sop >= sop.max_concurrent as usize {
            return false;
        }

        if self.active_runs.len() >= self.config.max_concurrent_total {
            return false;
        }

        if sop.cooldown_secs > 0 {
            if let Some(last) = self.last_finished_run(sop_name) {
                if let Some(ref completed_at) = last.completed_at {
                    if !cooldown_elapsed(completed_at, sop.cooldown_secs) {
                        return false;
                    }
                }
            }
        }

        true
    }

    pub fn start_run(&mut self, sop_name: &str, event: SopEvent) -> Result<SopRunAction> {

        if self.get_sop(sop_name).map_or(false, |s| {
            s.execution_mode == SopExecutionMode::Deterministic
        }) {
            return self.start_deterministic_run(sop_name, event);
        }

        let sop = self
            .get_sop(sop_name)
            .ok_or_else(|| anyhow::anyhow!("SOP not found: {sop_name}"))?
            .clone();

        if !self.can_start(sop_name) {
            bail!(
                "Cannot start SOP '{}': cooldown or concurrency limit reached",
                sop_name
            );
        }

        if sop.steps.is_empty() {
            bail!("SOP '{}' has no steps defined", sop_name);
        }

        self.run_counter += 1;
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let epoch_ms = dur.as_secs() * 1000 + u64::from(dur.subsec_millis());
        let run_id = format!("run-{epoch_ms}-{:04}", self.run_counter);
        let now = now_iso8601();

        let run = SopRun {
            run_id: run_id.clone(),
            sop_name: sop_name.to_string(),
            trigger_event: event,
            status: SopRunStatus::Running,
            current_step: 1,
            total_steps: u32::try_from(sop.steps.len()).unwrap_or(u32::MAX),
            started_at: now,
            completed_at: None,
            step_results: Vec::new(),
            waiting_since: None,
            llm_calls_saved: 0,
        };

        self.active_runs.insert(run_id.clone(), run);

        info!("SOP run {} started for '{}'", run_id, sop_name);

        let step = sop.steps[0].clone();
        let context = format_step_context(&sop, &self.active_runs[&run_id], &step);
        let action = resolve_step_action(&sop, &step, run_id.clone(), context);

        if matches!(action, SopRunAction::WaitApproval { .. }) {
            if let Some(run) = self.active_runs.get_mut(&run_id) {
                run.status = SopRunStatus::WaitingApproval;
                run.waiting_since = Some(now_iso8601());
            }
        }

        Ok(action)
    }

    pub fn advance_step(&mut self, run_id: &str, result: SopStepResult) -> Result<SopRunAction> {
        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::anyhow!("Active run not found: {run_id}"))?;

        let sop = self
            .sops
            .iter()
            .find(|s| s.name == run.sop_name)
            .ok_or_else(|| anyhow::anyhow!("SOP '{}' no longer loaded", run.sop_name))?
            .clone();

        run.step_results.push(result.clone());

        if result.status == SopStepStatus::Failed {
            let reason = format!("Step {} failed: {}", result.step_number, result.output);
            warn!("SOP run {run_id}: {reason}");
            return Ok(self.finish_run(run_id, SopRunStatus::Failed, Some(reason)));
        }

        let next_step_num = run.current_step + 1;
        if next_step_num > run.total_steps {

            info!("SOP run {run_id} completed successfully");
            return Ok(self.finish_run(run_id, SopRunStatus::Completed, None));
        }

        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::anyhow!("Active run vanished while advancing: {run_id}"))?;
        run.current_step = next_step_num;

        let step_idx = (next_step_num - 1) as usize;
        let step = sop.steps[step_idx].clone();
        let context = format_step_context(&sop, run, &step);
        let run_id_str = run_id.to_string();
        let action = resolve_step_action(&sop, &step, run_id_str.clone(), context);

        if matches!(action, SopRunAction::WaitApproval { .. }) {
            if let Some(run) = self.active_runs.get_mut(&run_id_str) {
                run.status = SopRunStatus::WaitingApproval;
                run.waiting_since = Some(now_iso8601());
            }
        }

        Ok(action)
    }

    pub fn cancel_run(&mut self, run_id: &str) -> Result<()> {
        if !self.active_runs.contains_key(run_id) {
            bail!("Active run not found: {run_id}");
        }
        self.finish_run(run_id, SopRunStatus::Cancelled, None);
        info!("SOP run {run_id} cancelled");
        Ok(())
    }

    pub fn approve_step(&mut self, run_id: &str) -> Result<SopRunAction> {
        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::anyhow!("Active run not found: {run_id}"))?;

        if run.status != SopRunStatus::WaitingApproval {
            bail!(
                "Run {run_id} is not waiting for approval (status: {})",
                run.status
            );
        }

        run.status = SopRunStatus::Running;
        run.waiting_since = None;

        let sop = self
            .sops
            .iter()
            .find(|s| s.name == run.sop_name)
            .ok_or_else(|| anyhow::anyhow!("SOP '{}' no longer loaded", run.sop_name))?
            .clone();

        let step_idx = (run.current_step - 1) as usize;
        let step = sop.steps[step_idx].clone();
        let context = format_step_context(&sop, run, &step);

        Ok(SopRunAction::ExecuteStep {
            run_id: run_id.to_string(),
            step,
            context,
        })
    }

    pub fn finished_runs(&self, sop_name: Option<&str>) -> Vec<&SopRun> {
        self.finished_runs
            .iter()
            .filter(|r| sop_name.map_or(true, |name| r.sop_name == name))
            .collect()
    }

    pub fn deterministic_savings(&self) -> &DeterministicSavings {
        &self.deterministic_savings
    }

    pub fn start_deterministic_run(
        &mut self,
        sop_name: &str,
        event: SopEvent,
    ) -> Result<SopRunAction> {
        let sop = self
            .get_sop(sop_name)
            .ok_or_else(|| anyhow::anyhow!("SOP not found: {sop_name}"))?
            .clone();

        if sop.execution_mode != SopExecutionMode::Deterministic {
            bail!(
                "SOP '{}' is not in deterministic mode (mode: {})",
                sop_name,
                sop.execution_mode
            );
        }

        if !self.can_start(sop_name) {
            bail!(
                "Cannot start SOP '{}': cooldown or concurrency limit reached",
                sop_name
            );
        }

        if sop.steps.is_empty() {
            bail!("SOP '{}' has no steps defined", sop_name);
        }

        self.run_counter += 1;
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let epoch_ms = dur.as_secs() * 1000 + u64::from(dur.subsec_millis());
        let run_id = format!("det-{epoch_ms}-{:04}", self.run_counter);
        let now = now_iso8601();

        let total_steps = u32::try_from(sop.steps.len()).unwrap_or(u32::MAX);
        let run = SopRun {
            run_id: run_id.clone(),
            sop_name: sop_name.to_string(),
            trigger_event: event,
            status: SopRunStatus::Running,
            current_step: 1,
            total_steps,
            started_at: now,
            completed_at: None,
            step_results: Vec::new(),
            waiting_since: None,
            llm_calls_saved: 0,
        };

        self.active_runs.insert(run_id.clone(), run);
        info!(
            "Deterministic SOP run {} started for '{}'",
            run_id, sop_name
        );

        let step = sop.steps[0].clone();
        let input = serde_json::Value::Null;
        self.resolve_deterministic_action(&sop, &run_id, &step, input)
    }

    pub fn advance_deterministic_step(
        &mut self,
        run_id: &str,
        step_output: serde_json::Value,
    ) -> Result<SopRunAction> {
        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| anyhow::anyhow!("Active run not found: {run_id}"))?;

        let sop = self
            .sops
            .iter()
            .find(|s| s.name == run.sop_name)
            .ok_or_else(|| anyhow::anyhow!("SOP '{}' no longer loaded", run.sop_name))?
            .clone();

        let now = now_iso8601();
        let step_result = SopStepResult {
            step_number: run.current_step,
            status: SopStepStatus::Completed,
            output: step_output.to_string(),
            started_at: run.started_at.clone(),
            completed_at: Some(now),
        };
        run.step_results.push(step_result);

        run.llm_calls_saved += 1;

        let next_step_num = run.current_step + 1;
        if next_step_num > run.total_steps {
            info!(
                "Deterministic SOP run {run_id} completed ({} LLM calls saved)",
                run.llm_calls_saved
            );
            let saved = run.llm_calls_saved;
            self.deterministic_savings.total_llm_calls_saved += saved;
            self.deterministic_savings.total_runs += 1;
            return Ok(self.finish_run(run_id, SopRunStatus::Completed, None));
        }

        let run = self
            .active_runs
            .get_mut(run_id)
            .ok_or_else(|| {
                anyhow::anyhow!("Active deterministic run vanished while advancing: {run_id}")
            })?;
        run.current_step = next_step_num;

        let step_idx = (next_step_num - 1) as usize;
        let step = sop.steps[step_idx].clone();
        let run_id_owned = run_id.to_string();

        self.resolve_deterministic_action(&sop, &run_id_owned, &step, step_output)
    }

    pub fn resume_deterministic_run(
        &mut self,
        state: DeterministicRunState,
    ) -> Result<SopRunAction> {
        let run = self
            .active_runs
            .get_mut(&state.run_id)
            .ok_or_else(|| anyhow::anyhow!("Active run not found: {}", state.run_id))?;

        if run.status != SopRunStatus::PausedCheckpoint {
            bail!(
                "Run {} is not paused at checkpoint (status: {})",
                state.run_id,
                run.status
            );
        }

        let sop = self
            .sops
            .iter()
            .find(|s| s.name == run.sop_name)
            .ok_or_else(|| anyhow::anyhow!("SOP '{}' no longer loaded", run.sop_name))?
            .clone();

        run.status = SopRunStatus::Running;
        run.waiting_since = None;
        run.llm_calls_saved = state.llm_calls_saved;

        let next_step_num = state.last_completed_step + 1;
        if next_step_num > state.total_steps {
            info!(
                "Deterministic SOP run {} completed on resume ({} LLM calls saved)",
                state.run_id, state.llm_calls_saved
            );
            self.deterministic_savings.total_llm_calls_saved += state.llm_calls_saved;
            self.deterministic_savings.total_runs += 1;
            return Ok(self.finish_run(&state.run_id, SopRunStatus::Completed, None));
        }

        let run = self
            .active_runs
            .get_mut(&state.run_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Active deterministic run vanished while resuming: {}",
                    state.run_id
                )
            })?;
        run.current_step = next_step_num;

        let step_idx = (next_step_num - 1) as usize;
        let step = sop.steps[step_idx].clone();

        let last_output = state
            .step_outputs
            .get(&state.last_completed_step)
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let run_id = state.run_id.clone();
        self.resolve_deterministic_action(&sop, &run_id, &step, last_output)
    }

    fn resolve_deterministic_action(
        &mut self,
        sop: &Sop,
        run_id: &str,
        step: &SopStep,
        input: serde_json::Value,
    ) -> Result<SopRunAction> {
        if step.kind == SopStepKind::Checkpoint {

            if let Some(run) = self.active_runs.get_mut(run_id) {
                run.status = SopRunStatus::PausedCheckpoint;
                run.waiting_since = Some(now_iso8601());
            }

            let state_file = self.persist_deterministic_state(run_id, sop)?;

            info!(
                "Deterministic SOP run {run_id}: checkpoint at step {} '{}', state persisted to {}",
                step.number,
                step.title,
                state_file.display()
            );

            Ok(SopRunAction::CheckpointWait {
                run_id: run_id.to_string(),
                step: step.clone(),
                state_file,
            })
        } else {
            Ok(SopRunAction::DeterministicStep {
                run_id: run_id.to_string(),
                step: step.clone(),
                input,
            })
        }
    }

    fn persist_deterministic_state(&self, run_id: &str, sop: &Sop) -> Result<PathBuf> {
        let run = self
            .active_runs
            .get(run_id)
            .ok_or_else(|| anyhow::anyhow!("Run not found: {run_id}"))?;

        let mut step_outputs = HashMap::new();
        for result in &run.step_results {

            let value = serde_json::from_str(&result.output)
                .unwrap_or_else(|_| serde_json::Value::String(result.output.clone()));
            step_outputs.insert(result.step_number, value);
        }

        let state = DeterministicRunState {
            run_id: run_id.to_string(),
            sop_name: run.sop_name.clone(),
            last_completed_step: run.current_step.saturating_sub(1),
            total_steps: run.total_steps,
            step_outputs,
            persisted_at: now_iso8601(),
            llm_calls_saved: run.llm_calls_saved,
            paused_at_checkpoint: run.status == SopRunStatus::PausedCheckpoint,
        };

        let temp_dir = std::env::temp_dir();
        let dir = sop
            .location
            .as_deref()
            .unwrap_or(temp_dir.as_path());
        let state_file = dir.join(format!("{run_id}.state.json"));
        let json = serde_json::to_string_pretty(&state)?;
        std::fs::write(&state_file, json)?;

        Ok(state_file)
    }

    pub fn load_deterministic_state(path: &Path) -> Result<DeterministicRunState> {
        let content = std::fs::read_to_string(path)?;
        let state: DeterministicRunState = serde_json::from_str(&content)?;
        Ok(state)
    }

    pub fn check_approval_timeouts(&mut self) -> Vec<SopRunAction> {
        let timeout_secs = self.config.approval_timeout_secs;
        if timeout_secs == 0 {
            return Vec::new();
        }

        let timed_out: Vec<(String, bool)> = self
            .active_runs
            .values()
            .filter(|r| r.status == SopRunStatus::WaitingApproval)
            .filter(|r| {
                r.waiting_since
                    .as_deref()
                    .map_or(false, |ts| cooldown_elapsed(ts, timeout_secs))
            })
            .map(|r| {
                let is_critical = self
                    .sops
                    .iter()
                    .find(|s| s.name == r.sop_name)
                    .map_or(false, |s| {
                        matches!(s.priority, SopPriority::Critical | SopPriority::High)
                    });
                (r.run_id.clone(), is_critical)
            })
            .collect();

        let mut actions = Vec::new();
        for (run_id, is_critical) in timed_out {
            if is_critical {

                info!(
                    "SOP run {run_id}: approval timeout  -  auto-approving (critical/high priority)"
                );
                match self.approve_step(&run_id) {
                    Ok(action) => actions.push(action),
                    Err(e) => warn!("SOP run {run_id}: auto-approve failed: {e}"),
                }
            } else {
                info!(
                    "SOP run {run_id}: approval timeout  -  cancelling run (non-critical) to \
                     release its concurrency slot"
                );
                actions.push(self.finish_run(
                    &run_id,
                    SopRunStatus::Cancelled,
                    Some("approval timeout".to_string()),
                ));
            }
        }

        actions
    }

    pub const MAX_RUN_LIFETIME_SECS: u64 = 24 * 60 * 60;

    pub fn reap_stale_runs(&mut self, max_age_secs: u64) -> Vec<SopRunAction> {
        if max_age_secs == 0 {
            return Vec::new();
        }
        let stale: Vec<String> = self
            .active_runs
            .values()
            .filter(|r| cooldown_elapsed(&r.started_at, max_age_secs))
            .map(|r| r.run_id.clone())
            .collect();

        let mut actions = Vec::new();
        for run_id in stale {
            warn!(
                "SOP run {run_id}: exceeded max lifetime ({max_age_secs}s) without reaching a \
                 terminal state; marking failed to release its concurrency slot"
            );
            actions.push(self.finish_run(
                &run_id,
                SopRunStatus::Failed,
                Some(format!("run exceeded max lifetime of {max_age_secs}s")),
            ));
        }
        actions
    }

    fn last_finished_run(&self, sop_name: &str) -> Option<&SopRun> {
        self.finished_runs
            .iter()
            .rev()
            .find(|r| r.sop_name == sop_name)
    }

    fn finish_run(
        &mut self,
        run_id: &str,
        status: SopRunStatus,
        reason: Option<String>,
    ) -> SopRunAction {
        let Some(mut run) = self.active_runs.remove(run_id) else {
            tracing::warn!(
                "SOP finish_run called for missing run \"{run_id}\"; returning Failed action"
            );
            return SopRunAction::Failed {
                run_id: run_id.to_string(),
                sop_name: String::new(),
                reason: reason.unwrap_or_else(|| "active run missing".to_string()),
            };
        };
        run.status = status;
        run.completed_at = Some(now_iso8601());
        let sop_name = run.sop_name.clone();
        let run_id_owned = run.run_id.clone();
        self.finished_runs.push(run);

        let max = self.config.max_finished_runs;
        if max > 0 && self.finished_runs.len() > max {
            let excess = self.finished_runs.len() - max;
            self.finished_runs.drain(..excess);
        }

        match status {
            SopRunStatus::Failed => SopRunAction::Failed {
                run_id: run_id_owned,
                sop_name,
                reason: reason.unwrap_or_default(),
            },
            _ => SopRunAction::Completed {
                run_id: run_id_owned,
                sop_name,
            },
        }
    }
}

fn trigger_matches(trigger: &SopTrigger, event: &SopEvent) -> bool {
    match (trigger, event.source) {
        (SopTrigger::Mqtt { topic, condition }, SopTriggerSource::Mqtt) => {
            let topic_match = event
                .topic
                .as_deref()
                .map_or(false, |t| mqtt_topic_matches(topic, t));
            if !topic_match {
                return false;
            }

            match condition {
                Some(cond) => evaluate_condition(cond, event.payload.as_deref()),
                None => true,
            }
        }

        (SopTrigger::Webhook { path }, SopTriggerSource::Webhook) => {
            event.topic.as_deref().map_or(false, |t| t == path)
        }

        (
            SopTrigger::Peripheral {
                board,
                signal,
                condition,
            },
            SopTriggerSource::Peripheral,
        ) => {
            let topic_match = event.topic.as_deref().map_or(false, |t| {
                let expected = format!("{board}/{signal}");
                t == expected
            });
            if !topic_match {
                return false;
            }

            match condition {
                Some(cond) => evaluate_condition(cond, event.payload.as_deref()),
                None => true,
            }
        }

        (SopTrigger::Cron { expression }, SopTriggerSource::Cron) => {
            event.topic.as_deref().map_or(false, |t| t == expression)
        }

        (SopTrigger::Manual, SopTriggerSource::Manual) => true,

        _ => false,
    }
}

fn mqtt_topic_matches(pattern: &str, topic: &str) -> bool {
    let pat_parts: Vec<&str> = pattern.split('/').collect();
    let top_parts: Vec<&str> = topic.split('/').collect();

    let mut pi = 0;
    let mut ti = 0;

    while pi < pat_parts.len() && ti < top_parts.len() {
        match pat_parts[pi] {
            "#" => return true,
            "+" => {

                pi += 1;
                ti += 1;
            }
            seg => {
                if seg != top_parts[ti] {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }

    pi == pat_parts.len() && ti == top_parts.len()
}

fn resolve_step_action(sop: &Sop, step: &SopStep, run_id: String, context: String) -> SopRunAction {

    if step.requires_confirmation {
        return SopRunAction::WaitApproval {
            run_id,
            step: step.clone(),
            context,
        };
    }

    let needs_approval = match sop.execution_mode {

        SopExecutionMode::Auto | SopExecutionMode::Deterministic => false,
        SopExecutionMode::Supervised => {

            step.number == 1
        }
        SopExecutionMode::StepByStep => true,
        SopExecutionMode::PriorityBased => match sop.priority {
            SopPriority::Critical | SopPriority::High => false,
            SopPriority::Normal | SopPriority::Low => {

                step.number == 1
            }
        },
    };

    if needs_approval {
        SopRunAction::WaitApproval {
            run_id,
            step: step.clone(),
            context,
        }
    } else {
        SopRunAction::ExecuteStep {
            run_id,
            step: step.clone(),
            context,
        }
    }
}

fn format_step_context(sop: &Sop, run: &SopRun, step: &SopStep) -> String {
    let mut ctx = format!(
        "[SOP: {} (run {})  -  Step {} of {}]\n\n",
        sop.name, run.run_id, step.number, run.total_steps
    );

    let _ = writeln!(
        ctx,
        "Trigger: {} {}",
        run.trigger_event.source,
        run.trigger_event.topic.as_deref().unwrap_or("(no topic)")
    );

    if let Some(ref payload) = run.trigger_event.payload {
        let _ = writeln!(ctx, "Payload: {payload}");
    }

    if let Some(prev) = run.step_results.last() {
        let _ = writeln!(
            ctx,
            "Previous: Step {} {}  -  {}",
            prev.step_number, prev.status, prev.output
        );
    }

    let _ = write!(ctx, "\nCurrent step: **{}**\n{}\n", step.title, step.body);

    if !step.suggested_tools.is_empty() {
        let _ = write!(
            ctx,
            "\nSuggested tools: {}\n",
            step.suggested_tools.join(", ")
        );
    }

    ctx.push_str("\nWhen done, report your result.\n");

    ctx
}

pub(crate) fn now_iso8601() -> String {

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {

    days += 719_468;
    let era = days / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn cooldown_elapsed(completed_at: &str, cooldown_secs: u64) -> bool {

    let completed = parse_iso8601_secs(completed_at);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match completed {
        Some(ts) => now.saturating_sub(ts) >= cooldown_secs,
        None => true,
    }
}

fn parse_iso8601_secs(input: &str) -> Option<u64> {

    let input = input.trim_end_matches('Z');
    let parts: Vec<&str> = input.split('T').collect();
    if parts.len() != 2 {
        return None;
    }
    let date_parts: Vec<u64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    let time_parts: Vec<u64> = parts[1].split(':').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return None;
    }
    let (year, month, day) = (date_parts[0], date_parts[1], date_parts[2]);
    let (hour, min, sec) = (time_parts[0], time_parts[1], time_parts[2]);

    let year_adj = if month <= 2 { year - 1 } else { year };
    let month_adj = if month > 2 { month - 3 } else { month + 9 };
    let era = year_adj / 400;
    let yoe = year_adj - era * 400;
    let doy = (153 * month_adj + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}
