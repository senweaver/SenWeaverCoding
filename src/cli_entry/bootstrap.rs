// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! CLI bootstrap helpers extracted from `main.rs`.
//!
//! Keeping these small helpers in the library crate means:
//! 1. The future `bin/sen_cli/main.rs` shrinks to a trivial
//!    dispatcher.
//! 2. Unit tests can exercise bootstrap logic without spinning up
//!    the full clap argument parser.
//! 3. Downstream embedders (future gateway-only binary, test
//!    harnesses) can reuse the exact same `.env` discovery logic.
//!
//! Anti-placeholder: `main.rs::load_env` delegates to
//! [`load_env`] below — grep `cli_entry::bootstrap::load_env`
//! in `src/main.rs` to confirm the real call site.

use std::io::Write;
use std::path::PathBuf;

pub fn load_env() {
    let candidates = [
        std::env::var_os("SEN_WORKSPACE")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| p.join(".env")),
        std::env::current_dir().ok().map(|p| p.join(".env")),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(".env"))),
    ]
    .into_iter()
    .flatten()
    .filter(|p| p.is_file());

    for path in candidates {
        if dotenvy::from_path(&path).is_ok() {
            tracing::debug!(path = %path.display(), "Loaded env file");
        }
    }
}

pub fn parse_temperature(s: &str) -> std::result::Result<f64, String> {
    let t: f64 = s
        .parse()
        .map_err(|e: std::num::ParseFloatError| format!("{e}"))?;
    crate::config::schema::validate_temperature(t)
}

pub fn print_no_command_help() -> anyhow::Result<()> {
    println!("SenWeaverCoding — AI Code Editor\n");
    println!("Usage:");
    println!("  sen                          Start interactive session");
    println!("  sen \"explain this code\"      Start with initial prompt");
    println!("  sen -p \"summarize\"           One-shot print mode");
    println!("  sen -c                       Continue last conversation");
    println!("  sen onboard                  First-time setup");
    println!("  sen --help                   Show all commands");
    println!();
    println!("Run `sen onboard` if this is your first time.");

    #[cfg(windows)]
    pause_after_no_command_help();

    Ok(())
}

#[cfg(windows)]
fn pause_after_no_command_help() {
    println!();
    print!("Press Enter to exit...");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn _keep_write_trait_imported(_w: &mut dyn Write) {}
