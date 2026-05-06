// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const MAX_PLAN_EXECUTION_NUDGES: usize = 3;

pub const INLINE_PROGRESS_REMINDER_INTERVAL: usize = 6;

pub const MAX_INLINE_PROGRESS_REMINDERS: usize = 12;

#[derive(Debug, Default, Clone)]
pub struct PlanExecutionNudgeState {

    pub active: bool,

    pub plan_path: Option<String>,

    pub total_steps: usize,

    pub terminal_count: usize,

    pub nudge_count: usize,

    pub last_update_iter: Option<usize>,

    pub inline_reminder_count: usize,
}

impl PlanExecutionNudgeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn armed(plan_path: String) -> Self {
        Self {
            active: true,
            plan_path: Some(plan_path),
            ..Self::default()
        }
    }

    pub fn observe_update_plan_call(
        &mut self,
        tool_name: &str,
        arguments: &serde_json::Value,
        output: &str,
        success: bool,
    ) {
        self.observe_update_plan_call_at(tool_name, arguments, output, success, None);
    }

    pub fn observe_update_plan_call_at(
        &mut self,
        tool_name: &str,
        arguments: &serde_json::Value,
        output: &str,
        success: bool,
        iteration: Option<usize>,
    ) {
        if !self.active || !success || tool_name != "update_plan" {
            return;
        }
        if let Some(iter) = iteration {
            self.last_update_iter = Some(iter);
        }
        let action = arguments
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match action {
            "set" => {
                if let Some(steps) = arguments.get("steps").and_then(|v| v.as_array()) {
                    self.total_steps = steps.len();

                    self.terminal_count = 0;
                }
            }
            "update" => {
                let status = arguments
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if matches!(status, "completed" | "skipped") {
                    self.terminal_count = self.terminal_count.saturating_add(1);
                }
            }
            "get" => {

                let mut total = 0usize;
                let mut terminal = 0usize;
                for line in output.lines() {
                    let trimmed = line.trim_start();
                    let checkbox = if trimmed.starts_with("- [ ]") {
                        Some(false)
                    } else if trimmed.starts_with("- [~]") {
                        Some(false)
                    } else if trimmed.starts_with("- [x]") {
                        Some(true)
                    } else if trimmed.starts_with("- [-]") {
                        Some(true)
                    } else {
                        None
                    };
                    if let Some(is_terminal) = checkbox {
                        total += 1;
                        if is_terminal {
                            terminal += 1;
                        }
                    }
                }
                if total > 0 {
                    self.total_steps = total;
                    self.terminal_count = terminal;
                }
            }
            _ => {}
        }
    }

    pub fn remaining(&self) -> usize {
        self.total_steps.saturating_sub(self.terminal_count)
    }

    pub fn inline_progress_reminder_due(&self, current_iter: usize) -> bool {
        if !self.active || self.total_steps == 0 {
            return false;
        }
        if self.terminal_count >= self.total_steps {
            return false;
        }
        if self.inline_reminder_count >= MAX_INLINE_PROGRESS_REMINDERS {
            return false;
        }
        let last = self.last_update_iter.unwrap_or(0);
        current_iter.saturating_sub(last) >= INLINE_PROGRESS_REMINDER_INTERVAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanExecutionExitDecision {

    Allow,

    InjectNudge,
}

pub fn evaluate_plan_execution_exit(
    state: &PlanExecutionNudgeState,
    awaiting_user_input: bool,
) -> PlanExecutionExitDecision {
    if awaiting_user_input {
        return PlanExecutionExitDecision::Allow;
    }
    if !state.active {
        return PlanExecutionExitDecision::Allow;
    }

    if state.total_steps == 0 {
        return PlanExecutionExitDecision::Allow;
    }
    if state.terminal_count >= state.total_steps {
        return PlanExecutionExitDecision::Allow;
    }
    if state.nudge_count >= MAX_PLAN_EXECUTION_NUDGES {
        tracing::warn!(
            target: "agent.plan_execution",
            nudge_count = state.nudge_count,
            total_steps = state.total_steps,
            terminal_count = state.terminal_count,
            "Plan execution nudging beyond soft cap; \
             check model / prompt conformance"
        );
    }
    PlanExecutionExitDecision::InjectNudge
}

pub fn nudge_message(state: &PlanExecutionNudgeState) -> String {
    let done = state.terminal_count;
    let total = state.total_steps;
    let remaining = state.remaining();
    let plan_ref = state
        .plan_path
        .as_deref()
        .map(|p| format!(" (plan: `{p}`)"))
        .unwrap_or_default();

    if state.nudge_count >= MAX_PLAN_EXECUTION_NUDGES {
        format!(
            "[Plan Execution — CRITICAL] You stopped at {done}/{total} with {remaining} \
             todo(s) still pending or in_progress{plan_ref}. This is your final warning. \
             You are in the middle of a plan-execution turn — you MUST NOT stop until \
             every step is `completed` or `skipped`. Do NOT reply with free-form text. \
             Do NOT ask for confirmation. Your next tool call MUST be \
             `update_plan(action=\"update\", step_id=<next_pending_id>, \
             status=\"in_progress\")` followed by the actual work, then \
             `update_plan(action=\"update\", step_id=<same_id>, status=\"completed\")` \
             (or `\"skipped\"` with a `notes` reason). Repeat for every remaining \
             step, then run the `## 验收` / Verification commands before finishing."
        )
    } else {
        format!(
            "[Plan Execution Reminder] You ended the turn at {done}/{total}, but \
             {remaining} todo(s) are still pending or in_progress{plan_ref}. You are \
             still inside a plan-execution turn triggered by the user clicking **Build** — \
             you MUST keep going until every step reaches `completed` or `skipped`. \
             For each remaining step: call `update_plan(action=\"update\", step_id=<id>, \
             status=\"in_progress\")`, perform the edits / shell commands that step \
             requires, then call `update_plan(action=\"update\", step_id=<id>, \
             status=\"completed\")` (or `\"skipped\"` with a `notes` reason if the step \
             is no longer needed). If you've lost track of which step is next, call \
             `update_plan(action=\"get\")` first to inspect the current plan state. \
             Do NOT stop to ask for confirmation. Continue now."
        )
    }
}

pub fn inline_progress_reminder_message(state: &PlanExecutionNudgeState) -> String {
    let done = state.terminal_count;
    let total = state.total_steps;
    let remaining = state.remaining();
    let plan_ref = state
        .plan_path
        .as_deref()
        .map(|p| format!(" (plan: `{p}`)"))
        .unwrap_or_default();
    format!(
        "[Plan Sync — mid-turn check] Progress is stuck at {done}/{total} on the user's \
         live progress bar with {remaining} step(s) still pending or in_progress{plan_ref}. \
         Several tool calls have been issued without an `update_plan` call — this means \
         your real progress and the user's UI have desynced. \
         IMMEDIATELY before your very next non-`update_plan` tool call, you MUST: \
         (1) call `update_plan(action=\"update\", step_id=<id>, status=\"completed\")` \
             (or `\"skipped\"` with `notes`) for every step you have ALREADY finished but \
             not yet flipped on the tracker; \
         (2) call `update_plan(action=\"update\", step_id=<next_id>, status=\"in_progress\")` \
             for the step you are about to work on. \
         If you have lost track of which step is which, call `update_plan(action=\"get\")` \
         first to inspect the live tracker. Do NOT batch all status updates at the end of \
         the turn — the user is watching the bar move in real time."
    )
}
