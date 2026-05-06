// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod builtin;
mod hot_runner;
mod runner;
pub mod script_runner;
mod traits;

pub use hot_runner::{HotHookRunner, build_runner};
pub use runner::HookRunner;
pub use script_runner::{
    HookCommand, HookDecision, HookEvent, HookMatchers, HookPayload, HooksConfig,
    ScriptHookRunner, event_for_tool_post, event_for_tool_pre,
};

#[allow(unused_imports)]
pub use traits::{HookHandler, HookResult};
