// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// /clear command — mirrors claude-code-typescript-src`commands/clear/`.
// Clears the terminal screen.

use super::registry::{CommandContext, CommandResult};

pub async fn handle(_ctx: CommandContext) -> CommandResult {
    print!("\x1B[2J\x1B[H");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    CommandResult::ok("Screen cleared.")
}
