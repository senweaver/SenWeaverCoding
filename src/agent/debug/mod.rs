// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod params;
pub mod prompt;
pub mod submode;

pub use submode::DebugSubMode;

use serde_json::Value;

fn gateway_session_key(session_id: &str) -> String {
    format!("gw_{session_id}")
}

pub fn active_debug_selection() -> Option<(String, Value)> {
    let svc = crate::services::try_get_services()?;
    let session = crate::session::current_session_context()?;
    let selection = svc.session_debug(&gateway_session_key(&session.session_id))?;
    Some((selection.submode_id, selection.params))
}

pub fn active_debug_submode() -> DebugSubMode {
    active_debug_selection()
        .and_then(|(id, _)| DebugSubMode::from_id(&id))
        .unwrap_or(DebugSubMode::Auto)
}

pub fn debug_submode_addendum() -> String {
    let selection = active_debug_selection();
    let sub = selection
        .as_ref()
        .and_then(|(id, _)| DebugSubMode::from_id(id))
        .unwrap_or(DebugSubMode::Auto);

    if matches!(sub, DebugSubMode::Auto) {
        return String::new();
    }

    let mut out = String::new();
    out.push_str(&prompt::submode_header(sub));
    out.push_str(prompt::contract(sub));

    if let Some((_, params)) = selection.as_ref() {
        let block = params::render_params_prompt(sub, params);
        if !block.is_empty() {
            out.push('\n');
            out.push_str(&block);
        }
    }
    out.push('\n');
    out
}
