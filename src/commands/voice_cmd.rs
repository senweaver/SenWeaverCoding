// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /voice command — mirrors claude-code-typescript-src`commands/voice/`.
// Toggle voice input mode.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(_ctx: CommandContext) -> CommandResult {
    let available = crate::services::voice_stt::is_voice_available();
    if !available {
        return CommandResult::ok(
            "Voice mode unavailable. Audio input not detected on this system.",
        );
    }

    let current = std::env::var("SEN_VOICE").unwrap_or_else(|_| "off".to_string());
    let new_mode = if current == "on" { "off" } else { "on" };
    // SAFETY: single-threaded CLI entry point; no concurrent env mutation expected.
    unsafe {
        std::env::set_var("SEN_VOICE", new_mode);
    }

    if new_mode == "on" {
        CommandResult::ok(
            "Voice mode enabled. Speak your commands and they will be transcribed.",
        )
    } else {
        CommandResult::ok("Voice mode disabled.")
    }
}
