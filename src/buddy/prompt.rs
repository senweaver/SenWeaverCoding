// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub fn buddy_system_prompt(name: &str, personality: &str) -> String {
    format!(
        "Your companion name is {name}. \
         Your personality is {personality}. \
         Stay attentive and adapt your tone naturally to how the conversation is going."
    )
}
