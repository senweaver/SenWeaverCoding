// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Message-list helpers used by the turn engine.

use crate::providers::ChatMessage;

#[inline]
pub fn estimate_history_tokens(history: &[ChatMessage]) -> usize {
    crate::agent::loop_::estimate_history_tokens(history)
}

#[inline]
#[deprecated(
    since = "0.1.0",
    note = "Use crate::agent::context_expansion::expand_input instead."
)]
pub fn expand_at_file_references(input: &str, workspace: &std::path::Path) -> String {
    crate::agent::context_expansion::expand_input(
        input,
        workspace,
        Vec::new(),
        String::new(),
    )
}
