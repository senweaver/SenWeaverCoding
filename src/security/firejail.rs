// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Firejail sandbox (Linux user-space sandboxing)
//!
//! Firejail is a SUID sandbox program that Linux applications use to sandbox themselves.

use crate::security::traits::Sandbox;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct FirejailSandbox;

impl FirejailSandbox {

    pub fn new() -> std::io::Result<Self> {
        if Self::is_installed() {
            Ok(Self)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Firejail not found. Install with: sudo apt install firejail",
            ))
        }
    }

    pub fn probe() -> std::io::Result<Self> {
        Self::new()
    }

    fn is_installed() -> bool {
        Command::new("firejail")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Sandbox for FirejailSandbox {
    fn wrap_command(&self, cmd: &mut Command) -> std::io::Result<()> {

        let program = cmd.get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        let mut firejail_cmd = Command::new("firejail");
        firejail_cmd.args([
            "--private=home",
            "--private-dev",
            "--nosound",
            "--no3d",
            "--novideo",
            "--nowheel",
            "--notv",
            "--noprofile",
            "--quiet",
        ]);

        firejail_cmd.arg(&program);
        firejail_cmd.args(&args);

        *cmd = firejail_cmd;
        Ok(())
    }

    fn is_available(&self) -> bool {
        Self::is_installed()
    }

    fn name(&self) -> &str {
        "firejail"
    }

    fn description(&self) -> &str {
        "Linux user-space sandbox (requires firejail to be installed)"
    }
}
