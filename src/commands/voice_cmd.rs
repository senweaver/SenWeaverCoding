// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
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

#[cfg(not(feature = "voice-wake"))]
pub async fn handle_voice(_ctx: CommandContext) -> CommandResult {
    CommandResult::ok(
        "Voice input is not available in this build: the voice-wake feature is not compiled in. \
         Rebuild with `--features voice-wake` and configure `channels_config.voice_wake` to enable it.",
    )
}

#[cfg(feature = "voice-wake")]
pub async fn handle_voice(_ctx: CommandContext) -> CommandResult {
    if !crate::services::voice_stt::is_voice_available() {
        return CommandResult::ok(
            "Voice mode unavailable. Audio input not detected on this system.",
        );
    }

    let wake_configured = crate::config::Config::load_or_init()
        .await
        .map(|cfg| cfg.channels_config.voice_wake.is_some())
        .unwrap_or(false);
    if !wake_configured {
        return CommandResult::ok(
            "Voice input is compiled in but not configured: set `channels_config.voice_wake` \
             (wake word, thresholds) in your config, then restart channels to enable capture.",
        );
    }

    let current = crate::util::get_runtime_var("SEN_VOICE").unwrap_or_else(|| "off".to_string());
    let new_mode = if current == "on" { "off" } else { "on" };

    crate::util::set_runtime_var("SEN_VOICE", new_mode);

    if new_mode == "on" {
        CommandResult::ok(
            "Voice mode enabled. The prompt now shows a [VOICE] indicator; capture is handled by the voice_wake channel.",
        )
    } else {
        CommandResult::ok("Voice mode disabled.")
    }
}
