// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const MAX_CURATOR_NUDGES: usize = 4;

pub const CURATOR_MODE_NUDGE_MESSAGE: &str =
    "[Curator-Mode Enforcement] You ended your response without calling \
     `exit_curator_mode`. In Curator mode this is invalid  -  the deliverable \
     is a saved `.senweavercoding/curators/<slug>/final.md` + `impl_blueprint.md` \
     (+ `final.docx`).\n\n\
     Your next message MUST either (a) continue research with curator-allowed \
     tools (web_search / web_fetch / curator_deep_collect / workspace_deep_search / \
     curator_collect / file_read / file_write under `.senweavercoding/curators/<slug>/`) OR (b) call \
     `exit_curator_mode(final_content=…, impl_blueprint=…)` with the polished \
     Markdown bodies. Do NOT reply with free-form text. Do NOT call any tool \
     outside the Curator allowlist. Before exiting, you MUST have made at least \
     5 distinct `web_search` calls, fetched at least 8 long web pages (via \
     `curator_deep_collect` or `web_fetch`), and run at least 1 \
     `workspace_deep_search`  -  otherwise the document is too shallow and \
     `exit_curator_mode` will be rejected.";

pub const CURATOR_MODE_NUDGE_STRONG: &str =
    "[Curator-Mode Enforcement  -  CRITICAL] You have failed to land the Curator \
     document multiple times. This is your last warning. You are in Curator mode \
     and MUST call `exit_curator_mode` with the complete `final_content` and \
     `impl_blueprint` arguments RIGHT NOW. Do NOT add any other text or tool \
     calls before `exit_curator_mode`. Do NOT stop without exiting  -  Curator mode \
     ends only by `exit_curator_mode` (success or rejection).";

#[derive(Debug, Default, Clone, Copy)]
pub struct CuratorModeNudgeState {
    pub exit_curator_mode_called: bool,
    pub nudge_count: usize,
}

impl CuratorModeNudgeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn note_exit_curator_mode_success(&mut self) {
        self.exit_curator_mode_called = true;
    }
}

pub fn detect_curator_mode_active(
    curator_mode_flag: Option<&crate::tools::curator::tools::CuratorModeFlag>,
) -> bool {
    let from_flag = curator_mode_flag.map(|f| f.is_active()).unwrap_or(false);
    if from_flag {
        return true;
    }
    matches!(
        crate::agent::coding_mode::active_coding_mode(),
        crate::agent::coding_mode::CodingMode::Curator
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuratorModeExitDecision {
    Allow,
    InjectNudge,
}

pub fn evaluate_curator_mode_exit(
    in_curator_mode: bool,
    state: &CuratorModeNudgeState,
    awaiting_user_input: bool,
) -> CuratorModeExitDecision {
    if awaiting_user_input {
        return CuratorModeExitDecision::Allow;
    }
    if !in_curator_mode || state.exit_curator_mode_called {
        return CuratorModeExitDecision::Allow;
    }
    if state.nudge_count >= MAX_CURATOR_NUDGES {
        tracing::warn!(
            target: "agent.curator_mode",
            nudge_count = state.nudge_count,
            "Curator mode still nudging beyond soft cap; check provider/model conformance"
        );
    }
    CuratorModeExitDecision::InjectNudge
}

pub fn nudge_message(state: &CuratorModeNudgeState) -> &'static str {
    if state.nudge_count >= MAX_CURATOR_NUDGES {
        CURATOR_MODE_NUDGE_STRONG
    } else {
        CURATOR_MODE_NUDGE_MESSAGE
    }
}
