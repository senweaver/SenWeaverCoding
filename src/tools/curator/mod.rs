// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod docx;
pub mod git_reference;
pub mod local_reference;
pub mod reference_helpers;
pub mod state;
pub mod templates;
pub mod tools;

pub use git_reference::CuratorGitReferenceTool;
pub use local_reference::CuratorLocalReferenceTool;
pub use state::{
    new_curator_state, CuratorActive, CuratorState, CuratorTemplateKind, PendingCurator,
    new_pending_curator,
};
pub use tools::{
    CuratorCollectTool, CuratorDeepCollectTool, CuratorTemplateApplyTool, CuratorTemplateListTool,
    EnterCuratorModeTool, ExitCuratorModeTool,
};
