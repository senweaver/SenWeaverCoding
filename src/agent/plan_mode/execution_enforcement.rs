// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const MAX_PLAN_EXECUTION_NUDGES: usize = 3;

pub const INLINE_PROGRESS_REMINDER_INTERVAL: usize = 6;

pub const MAX_INLINE_PROGRESS_REMINDERS: usize = 12;

pub const MAX_PLAN_EXECUTION_NUDGES_HARD: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoFinalizeIntent {
    AssumeCompleted,
    AssumeSkipped,
}

pub fn detect_completion_claim(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    const EN_STRONG: &[&str] = &[
        "all done",
        "all complete",
        "all completed",
        "all finished",
        "all the fixes",
        "all fixes complete",
        "all fixes are complete",
        "all fixes applied",
        "all fixes have been applied",
        "all changes applied",
        "all changes have been applied",
        "all steps complete",
        "all steps completed",
        "all tasks complete",
        "all tasks completed",
        "everything is done",
        "everything is complete",
        "everything has been",
        "task complete",
        "task completed",
        "task is complete",
        "successfully completed",
        "successfully fixed",
        "successfully applied",
        "successfully implemented",
        "completed and verified",
        "fix complete",
        "fixes complete",
        "verification passed",
        "verification successful",
        "build succeeds",
        "build successful",
        "build passed",
        "tests pass",
        "tests passed",
        "tests are passing",
        "no errors",
        "zero errors",
        "ready to commit",
        "ready for review",
        "implementation complete",
        "refactor complete",
        "migration complete",
        "all four passed",
        "all five passed",
    ];
    for pat in EN_STRONG {
        if lower.contains(pat) {
            return true;
        }
    }
    const CN_STRONG: &[&str] = &[
        "全部完成",
        "全部修复",
        "全部修改",
        "全部修复完成",
        "全部修改完成",
        "全部完成并验证",
        "已全部完成",
        "已全部修复",
        "已完成全部",
        "已完成所有",
        "都已完成",
        "都完成了",
        "都已修复",
        "已经完成",
        "已经修复",
        "修复完成",
        "修改完成",
        "实施完成",
        "实现完成",
        "执行完成",
        "已经执行完",
        "全部成功",
        "全部通过",
        "验证通过",
        "验证成功",
        "全部修复并验证",
        "修复总结",
        "修改总结",
        "完成总结",
        "总结完成",
        "已完成并验证",
        "已完成且通过",
        "已完成验证",
        "所有修复已完成",
        "所有问题已修复",
        "所有改动",
        "所有任务",
        "可以提交",
        "可以合入",
        "无错误",
        "零错误",
        "通过验证",
    ];
    for pat in CN_STRONG {
        if text.contains(pat) {
            return true;
        }
    }
    false
}

pub fn classify_auto_finalize_intent(recent_assistant_text: &str) -> AutoFinalizeIntent {
    if detect_completion_claim(recent_assistant_text) {
        AutoFinalizeIntent::AssumeCompleted
    } else {
        AutoFinalizeIntent::AssumeSkipped
    }
}

#[derive(Debug, Default, Clone)]
pub struct PlanExecutionNudgeState {

    pub active: bool,

    pub plan_path: Option<String>,

    pub total_steps: usize,

    pub terminal_count: usize,

    pub nudge_count: usize,

    pub unproductive_nudges: usize,

    pub terminal_at_last_nudge: usize,

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
                    let incoming_terminal = steps
                        .iter()
                        .filter(|s| {
                            matches!(
                                s.get("status").and_then(|v| v.as_str()),
                                Some("completed") | Some("skipped")
                            )
                        })
                        .count();
                    let prior = self.terminal_count.min(self.total_steps);
                    self.terminal_count = incoming_terminal.max(prior);
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

    pub fn note_nudge_issued(&mut self) {
        if self.terminal_count > self.terminal_at_last_nudge {
            self.unproductive_nudges = 0;
        } else {
            self.unproductive_nudges = self.unproductive_nudges.saturating_add(1);
        }
        self.terminal_at_last_nudge = self.terminal_count;
        self.nudge_count = self.nudge_count.saturating_add(1);
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
    if state.unproductive_nudges >= MAX_PLAN_EXECUTION_NUDGES_HARD {
        tracing::warn!(
            target: "agent.plan_execution",
            nudge_count = state.nudge_count,
            unproductive_nudges = state.unproductive_nudges,
            total_steps = state.total_steps,
            terminal_count = state.terminal_count,
            "Plan execution: stalled with no progress across the hard nudge cap; \
             allowing exit and auto-finalizing remaining steps"
        );
        return PlanExecutionExitDecision::Allow;
    }
    if state.unproductive_nudges >= MAX_PLAN_EXECUTION_NUDGES {
        tracing::warn!(
            target: "agent.plan_execution",
            nudge_count = state.nudge_count,
            unproductive_nudges = state.unproductive_nudges,
            total_steps = state.total_steps,
            terminal_count = state.terminal_count,
            "Plan execution nudging beyond soft cap without progress; \
             check model / prompt conformance"
        );
    }
    PlanExecutionExitDecision::InjectNudge
}

pub fn should_auto_finalize_on_exit(state: &PlanExecutionNudgeState) -> bool {
    state.active
        && state.total_steps > 0
        && state.terminal_count < state.total_steps
        && state.nudge_count >= 1
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
    let nudges_left = MAX_PLAN_EXECUTION_NUDGES_HARD.saturating_sub(state.unproductive_nudges);

    if state.unproductive_nudges >= MAX_PLAN_EXECUTION_NUDGES_HARD {
        format!(
            "[Plan Execution  -  FINAL CHANCE] You stopped at {done}/{total} with {remaining} \
             todo(s) still `pending` or `in_progress`{plan_ref}. After this nudge the runtime \
             WILL stop sending reminders, exit the turn, and run an auto-finalize on whatever \
             you leave behind  -  if you really finished the work but forgot to flip the \
             tracker, that auto-finalize will infer status from your last summary text, which \
             is fragile and error-prone. To keep the plan honest, your VERY NEXT response \
             MUST be a tool call (no free-form text first). Use this exact recipe:\n\
             \n\
             1. `update_plan(action=\"get\")` -> read which step ids are still open.\n\
             2. For EACH unfinished step, in order:\n\
                - `update_plan(action=\"update\", step_id=<id>, status=\"completed\", \
                   notes=\"<one-line evidence>\")` if you already did the work above; OR\n\
                - `update_plan(action=\"update\", step_id=<id>, status=\"skipped\", \
                   notes=\"<why>\")` if it's no longer needed; OR\n\
                - `update_plan(action=\"update\", step_id=<id>, status=\"in_progress\")` then \
                   do the real edits/shell commands now and flip to `completed` immediately \
                   after.\n\
             3. Only after every remaining id is terminal may you write a summary.\n\
             \n\
             Do NOT repeat a previous summary. Do NOT ask for confirmation. Do NOT call \
             unrelated tools first. Close the loop on the tracker now."
        )
    } else if state.unproductive_nudges >= MAX_PLAN_EXECUTION_NUDGES {
        format!(
            "[Plan Execution  -  CRITICAL] You stopped at {done}/{total} with {remaining} \
             todo(s) still pending or in_progress{plan_ref}. You have {nudges_left} \
             reminder(s) left before the runtime gives up and auto-finalizes the rest. \
             You are in the middle of a plan-execution turn  -  you MUST NOT stop until \
             every step is `completed` or `skipped`. Do NOT reply with free-form text. \
             Do NOT ask for confirmation. Your next tool call MUST be \
             `update_plan(action=\"update\", step_id=<next_pending_id>, \
             status=\"in_progress\")` followed by the actual work, then \
             `update_plan(action=\"update\", step_id=<same_id>, status=\"completed\")` \
             (or `\"skipped\"` with a `notes` reason). Repeat for every remaining \
             step, then run the `## Verification` commands before finishing."
        )
    } else {
        format!(
            "[Plan Execution Reminder] You ended the turn at {done}/{total}, but \
             {remaining} todo(s) are still pending or in_progress{plan_ref}. You are \
             still inside a plan-execution turn triggered by the user clicking **Build**  -  \
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
        "[Plan Sync  -  mid-turn check] Progress is stuck at {done}/{total} on the user's \
         live progress bar with {remaining} step(s) still pending or in_progress{plan_ref}. \
         Several tool calls have been issued without an `update_plan` call  -  this means \
         your real progress and the user's UI have desynced. \
         IMMEDIATELY before your very next non-`update_plan` tool call, you MUST: \
         (1) call `update_plan(action=\"update\", step_id=<id>, status=\"completed\")` \
             (or `\"skipped\"` with `notes`) for every step you have ALREADY finished but \
             not yet flipped on the tracker; \
         (2) call `update_plan(action=\"update\", step_id=<next_id>, status=\"in_progress\")` \
             for the step you are about to work on. \
         If you have lost track of which step is which, call `update_plan(action=\"get\")` \
         first to inspect the live tracker. Do NOT batch all status updates at the end of \
         the turn  -  the user is watching the bar move in real time."
    )
}
