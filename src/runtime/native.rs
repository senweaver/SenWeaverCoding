// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::RuntimeAdapter;
use std::path::{Path, PathBuf};

pub struct NativeRuntime;

impl NativeRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl RuntimeAdapter for NativeRuntime {
    fn name(&self) -> &str {
        "native"
    }

    fn has_shell_access(&self) -> bool {
        true
    }

    fn has_filesystem_access(&self) -> bool {
        true
    }

    fn storage_path(&self) -> PathBuf {
        directories::UserDirs::new().map_or_else(
            || PathBuf::from(".senweavercoding"),
            |u| u.home_dir().join(".senweavercoding"),
        )
    }

    fn supports_long_running(&self) -> bool {
        true
    }

    fn build_shell_command(
        &self,
        command: &str,
        workspace_dir: &Path,
    ) -> anyhow::Result<tokio::process::Command> {
        #[cfg(not(target_os = "windows"))]
        {
            let mut process = tokio::process::Command::new("sh");
            process.arg("-c").arg(command).current_dir(workspace_dir);
            Ok(process)
        }

        #[cfg(target_os = "windows")]
        {
            let mut process = tokio::process::Command::new("cmd.exe");
            process.arg("/C").arg(command).current_dir(workspace_dir);
            Ok(process)
        }
    }
}
