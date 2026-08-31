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
        "tests passed",
        "implementation complete",
        "refactor complete",
        "migration complete",
        "all four passed",
        "all five passed",
    ];
    for pat in EN_STRONG {
        let mut search = 0usize;
        while let Some(rel) = lower[search..].find(pat) {
            let pos = search + rel;
            if !negation_near(&lower, pos, pat.len()) {
                return true;
            }
            search = pos + pat.len();
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
        "可以提交",
        "可以合入",
        "通过验证",
    ];
    for pat in CN_STRONG {
        let mut search = 0usize;
        while let Some(rel) = text[search..].find(pat) {
            let pos = search + rel;
            if !negation_near(text, pos, pat.len()) {
                return true;
            }
            search = pos + pat.len();
        }
    }
    false
}

const NEGATION_MARKERS: &[&str] = &[
    "not ", "n't", "cannot", "can not", "unable", "fail", "still ", "remaining",
    "remain ", "blocked", "pending", "except", " yet", "couldn", "won't", "without",
    "未", "没", "无法", "还没", "尚未", "仍", "不能", "失败", "还需", "还要", "剩余",
    "阻塞", "不通过", "除了", "但", "however",
];

fn negation_near(haystack: &str, match_start: usize, match_len: usize) -> bool {
    const WINDOW_CHARS: usize = 24;
    let mut before_start = match_start;
    let mut count = 0;
    while before_start > 0 && count < WINDOW_CHARS {
        before_start -= 1;
        while before_start > 0 && !haystack.is_char_boundary(before_start) {
            before_start -= 1;
        }
        count += 1;
    }
    let mut after_end = (match_start + match_len).min(haystack.len());
    let mut count = 0;
    while after_end < haystack.len() && count < WINDOW_CHARS {
        after_end += 1;
        while after_end < haystack.len() && !haystack.is_char_boundary(after_end) {
            after_end += 1;
        }
        count += 1;
    }
    let window = haystack[before_start..after_end].to_ascii_lowercase();
    NEGATION_MARKERS.iter().any(|n| window.contains(n))
}

pub fn classify_auto_finalize_intent(recent_assistant_text: &str) -> AutoFinalizeIntent {
    if detect_completion_claim(recent_assistant_text) {
        AutoFinalizeIntent::AssumeCompleted
    } else {
        AutoFinalizeIntent::AssumeSkipped
    }
}

fn parse_update_output(output: &str) -> Option<(String, String)> {
    if let Some(rest) = output.strip_prefix("Updated step '") {
        let idx = rest.rfind("': status=")?;
        let title = &rest[..idx];
        let status = rest[idx + "': status=".len()..].trim();
        return Some((title.to_string(), status.to_string()));
    }
    if let Some(rest) = output.strip_prefix("Plan todo '") {
        let idx = rest.rfind("' annotated (status=")?;
        let title = &rest[..idx];
        let status = rest[idx + "' annotated (status=".len()..]
            .trim_end_matches(['.', ')', ' ']);
        return Some((title.to_string(), status.to_string()));
    }
    None
}

#[derive(Debug, Default, Clone)]
pub struct PlanExecutionNudgeState {

    pub active: bool,

    pub plan_path: Option<String>,

    pub total_steps: usize,

    pub terminal_count: usize,

    pub terminal_ids: std::collections::HashSet<String>,

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
                    let mut ids = std::collections::HashSet::new();
                    let mut terminal_without_id = 0usize;
                    for s in steps {
                        let is_terminal = matches!(
                            s.get("status").and_then(|v| v.as_str()),
                            Some("completed") | Some("skipped")
                        );
                        if !is_terminal {
                            continue;
                        }
                        let key = s
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(crate::tools::update_plan::normalize_plan_key)
                            .filter(|k| !k.is_empty());
                        match key {
                            Some(key) => {
                                ids.insert(key);
                            }
                            None => terminal_without_id += 1,
                        }
                    }
                    self.terminal_count = (ids.len() + terminal_without_id).min(self.total_steps);
                    self.terminal_ids = ids;
                }
            }
            "update" => {
                let arg_status = arguments
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arg_key = arguments
                    .get("step_id")
                    .and_then(|v| v.as_str())
                    .map(crate::tools::update_plan::normalize_plan_key)
                    .filter(|k| !k.is_empty());
                let (key, status) = match parse_update_output(output) {
                    Some((title, out_status)) => {
                        let title_key = crate::tools::update_plan::normalize_plan_key(&title);
                        if title_key.is_empty() {
                            (arg_key, out_status)
                        } else {
                            (Some(title_key), out_status)
                        }
                    }
                    None => (arg_key, arg_status.to_string()),
                };
                let is_terminal = matches!(status.as_str(), "completed" | "skipped");
                match key {
                    Some(key) => {
                        if is_terminal {
                            if self.terminal_ids.insert(key) {
                                self.terminal_count = self.terminal_count.saturating_add(1);
                            }
                        } else if self.terminal_ids.remove(&key) {
                            self.terminal_count = self.terminal_count.saturating_sub(1);
                        }
                    }
                    None => {
                        if is_terminal && self.terminal_count < self.total_steps.max(1) {
                            self.terminal_count = self.terminal_count.saturating_add(1);
                        }
                    }
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
                    if terminal < self.terminal_ids.len() {
                        self.terminal_ids.clear();
                    }
                    self.terminal_count = terminal.min(total);
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
