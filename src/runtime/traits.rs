// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::path::{Path, PathBuf};

pub trait RuntimeAdapter: Send + Sync {

    fn name(&self) -> &str;

    fn has_shell_access(&self) -> bool;

    fn has_filesystem_access(&self) -> bool;

    fn storage_path(&self) -> PathBuf;

    fn supports_long_running(&self) -> bool;

    fn memory_budget(&self) -> u64 {
        0
    }

    fn build_shell_command(
        &self,
        command: &str,
        workspace_dir: &Path,
    ) -> anyhow::Result<tokio::process::Command>;
}
