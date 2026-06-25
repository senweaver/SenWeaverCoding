// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const MAX_PLAN_NUDGES: usize = 3;

pub const HARD_PLAN_NUDGE_LIMIT: usize = 9;

pub const ASK_QUESTION_WAIT_SENTINEL: &str = "__WAITING_FOR_USER_RESPONSE__";

pub const ASK_QUESTION_PAUSE_NOTICE: &str =
    "User has been asked the clarifying question(s) above. \
     The runtime will deliver the user's reply in the next turn. \
     Stop here - do NOT call any other tool, do NOT draft a plan \
     yet. Wait for the next user message and resume planning then.";

pub fn is_ask_question_pause(tool_name: &str, output: &str) -> bool {
    matches!(tool_name, "ask_question" | "AskQuestion") && output.trim() == ASK_QUESTION_WAIT_SENTINEL
}

pub const PLAN_MODE_NUDGE_MESSAGE: &str =
    "[Plan-Mode Enforcement] You ended your response \
     without calling `exit_plan_mode`. In Plan mode \
     this is invalid - your single deliverable is a \
     saved `.plan.md` document.\n\nYour next message \
     MUST be a tool call to `exit_plan_mode(plan_content=\"...\")` \
     containing the complete plan in the format described \
     in the system prompt (YAML frontmatter with name, \
     overview, todos[]; followed by a Markdown body with \
     ## Overview, ## Steps, ## Verification, ## Risks). \
     Do NOT reply with free-form text. Do NOT call any \
     other tool. Produce the plan now.";

pub const PLAN_MODE_NUDGE_STRONG: &str =
    "[Plan-Mode Enforcement - CRITICAL] You have failed to produce the plan \
     multiple times.  This is your absolute last warning.  You are in Plan \
     mode and MUST call `exit_plan_mode` with the full plan content RIGHT NOW. \
     Do NOT add any other text or tool calls before `exit_plan_mode`. \
     Do NOT stop. Do NOT ask more questions. Write the plan and call \
     `exit_plan_mode` immediately. (soft cap exceeded  - if you cannot \
     produce the plan, call ask_question for the missing info; you MUST \
     NOT terminate without exit_plan_mode)";

#[derive(Debug, Default, Clone, Copy)]
pub struct PlanModeNudgeState {

    pub exit_plan_mode_called: bool,

    pub nudge_count: usize,
}

impl PlanModeNudgeState {

    pub fn new() -> Self {
        Self::default()
    }

    pub fn note_exit_plan_mode_success(&mut self) {
        self.exit_plan_mode_called = true;
    }
}

pub fn detect_plan_mode_active(plan_mode_flag: Option<&crate::tools::PlanModeFlag>) -> bool {
    let from_flag = plan_mode_flag.map(crate::tools::PlanModeFlag::is_active).unwrap_or(false);
    if from_flag {
        return true;
    }
    matches!(
        crate::agent::coding_mode::active_coding_mode(),
        crate::agent::coding_mode::CodingMode::Plan
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanModeExitDecision {

    Allow,

    InjectNudge,
}

pub fn evaluate_plan_mode_exit(
    in_plan_mode: bool,
    state: &PlanModeNudgeState,
    awaiting_user_input: bool,
) -> PlanModeExitDecision {
    if awaiting_user_input {
        return PlanModeExitDecision::Allow;
    }
    if !in_plan_mode || state.exit_plan_mode_called {
        return PlanModeExitDecision::Allow;
    }

    if state.nudge_count >= HARD_PLAN_NUDGE_LIMIT {
        tracing::error!(
            target: "agent.plan_mode",
            nudge_count = state.nudge_count,
            "Plan mode nudging exceeded hard limit; stopping nudges to avoid a live-lock \
             (provider/model is not honoring exit_plan_mode)"
        );
        return PlanModeExitDecision::Allow;
    }

    if state.nudge_count >= MAX_PLAN_NUDGES {
        tracing::warn!(
            target: "agent.plan_mode",
            nudge_count = state.nudge_count,
            "Plan mode is still nudging beyond soft cap; \
             check provider/model conformance"
        );
    }
    PlanModeExitDecision::InjectNudge
}

pub fn nudge_message(state: &PlanModeNudgeState) -> &'static str {
    if state.nudge_count >= MAX_PLAN_NUDGES {
        PLAN_MODE_NUDGE_STRONG
    } else {
        PLAN_MODE_NUDGE_MESSAGE
    }
}

