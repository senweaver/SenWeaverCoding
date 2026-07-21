// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::RuntimeAdapter;
use crate::config::WasmRuntimeConfig;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WasmRuntime {
    config: WasmRuntimeConfig,
    workspace_dir: Option<PathBuf>,
}

impl WasmRuntime {

    pub fn new(config: WasmRuntimeConfig) -> Self {
        Self {
            config,
            workspace_dir: None,
        }
    }

    pub fn with_workspace(config: WasmRuntimeConfig, workspace_dir: PathBuf) -> Self {
        Self {
            config,
            workspace_dir: Some(workspace_dir),
        }
    }

    pub fn is_available() -> bool {
        cfg!(feature = "runtime-wasm")
    }

    pub fn validate_config(&self) -> Result<()> {
        if self.config.memory_limit_mb == 0 {
            bail!("runtime.wasm.memory_limit_mb must be > 0");
        }
        if self.config.memory_limit_mb > 4096 {
            bail!(
                "runtime.wasm.memory_limit_mb of {} exceeds the 4 GB safety limit for 32-bit WASM",
                self.config.memory_limit_mb
            );
        }
        if self.config.tools_dir.is_empty() {
            bail!("runtime.wasm.tools_dir cannot be empty");
        }

        if self.config.tools_dir.contains("..") {
            bail!("runtime.wasm.tools_dir must not contain '..' path traversal");
        }
        Ok(())
    }
}

impl RuntimeAdapter for WasmRuntime {
    fn name(&self) -> &str {
        "wasm"
    }

    fn has_shell_access(&self) -> bool {

        false
    }

    fn has_filesystem_access(&self) -> bool {
        self.config.allow_workspace_read || self.config.allow_workspace_write
    }

    fn storage_path(&self) -> PathBuf {
        self.workspace_dir.as_ref().map_or_else(
            || PathBuf::from(".senweavercoding"),
            |w| w.join(".senweavercoding"),
        )
    }

    fn supports_long_running(&self) -> bool {

        false
    }

    fn memory_budget(&self) -> u64 {
        self.config.memory_limit_mb.saturating_mul(1024 * 1024)
    }

    fn build_shell_command(
        &self,
        _command: &str,
        _workspace_dir: &Path,
    ) -> anyhow::Result<tokio::process::Command> {
        bail!(
            "WASM runtime does not support shell commands. \
             Switch to runtime.kind = \"native\" for shell access."
        )
    }
}
