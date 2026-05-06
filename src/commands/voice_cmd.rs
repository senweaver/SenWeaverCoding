// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /voice command — mirrors claude-code-typescript-src`commands/voice/`.
// Toggle voice input mode.

use super::registry::{CommandCategory, CommandContext, CommandResult, StaticSlashCommand};

inventory::submit!(StaticSlashCommand {
    name: "voice",
    aliases: &[],
    description: "Toggle voice input mode",
    usage: "/voice",
    category: CommandCategory::Session,
    hidden: false,
    requires_interactive: true,
    remote_safe: false,
    handler: make_handler!(handle_voice),
});

pub async fn handle_voice(_ctx: CommandContext) -> CommandResult {
    let available = crate::services::voice_stt::is_voice_available();
    if !available {
        return CommandResult::ok(
            "Voice mode unavailable. Audio input not detected on this system.",
        );
    }

    let current = std::env::var("SEN_VOICE").unwrap_or_else(|_| "off".to_string());
    let new_mode = if current == "on" { "off" } else { "on" };

    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("SEN_VOICE", new_mode);
    }

    if new_mode == "on" {
        CommandResult::ok("Voice mode enabled. Speak your commands and they will be transcribed.")
    } else {
        CommandResult::ok("Voice mode disabled.")
    }
}
