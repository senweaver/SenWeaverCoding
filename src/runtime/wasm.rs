// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::RuntimeAdapter;
use crate::config::WasmRuntimeConfig;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WasmRuntime {
    config: WasmRuntimeConfig,
    workspace_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WasmExecutionResult {

    pub stdout: String,

    pub stderr: String,

    pub exit_code: i32,

    pub fuel_consumed: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WasmCapabilities {

    pub read_workspace: bool,

    pub write_workspace: bool,

    pub allowed_hosts: Vec<String>,

    pub fuel_override: u64,

    pub memory_override_mb: u64,
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

    pub fn tools_dir(&self, workspace_dir: &Path) -> PathBuf {
        workspace_dir.join(&self.config.tools_dir)
    }

    pub fn default_capabilities(&self) -> WasmCapabilities {
        WasmCapabilities {
            read_workspace: self.config.allow_workspace_read,
            write_workspace: self.config.allow_workspace_write,
            allowed_hosts: self.config.allowed_hosts.clone(),
            fuel_override: 0,
            memory_override_mb: 0,
        }
    }

    pub fn effective_fuel(&self, caps: &WasmCapabilities) -> u64 {
        if caps.fuel_override > 0 {
            caps.fuel_override
        } else {
            self.config.fuel_limit
        }
    }

    pub fn effective_memory_bytes(&self, caps: &WasmCapabilities) -> u64 {
        let mb = if caps.memory_override_mb > 0 {
            caps.memory_override_mb
        } else {
            self.config.memory_limit_mb
        };
        mb.saturating_mul(1024 * 1024)
    }

    #[cfg(feature = "runtime-wasm")]
    pub fn execute_module(
        &self,
        module_name: &str,
        workspace_dir: &Path,
        caps: &WasmCapabilities,
    ) -> Result<WasmExecutionResult> {
        use wasmi::{Engine, Linker, Module, Store};

        let tools_path = self.tools_dir(workspace_dir);
        let module_path = tools_path.join(format!("{module_name}.wasm"));

        if !module_path.exists() {
            bail!(
                "WASM module not found: {} (looked in {})",
                module_name,
                tools_path.display()
            );
        }

        let wasm_bytes = std::fs::read(&module_path)
            .with_context(|| format!("Failed to read WASM module: {}", module_path.display()))?;

        if wasm_bytes.len() > 50 * 1024 * 1024 {
            bail!(
                "WASM module {} is {} MB — exceeds 50 MB safety limit",
                module_name,
                wasm_bytes.len() / (1024 * 1024)
            );
        }

        let mut engine_config = wasmi::Config::default();
        engine_config.consume_fuel(true);
        let engine = Engine::new(&engine_config);

        let module = Module::new(&engine, &wasm_bytes[..])
            .with_context(|| format!("Failed to parse WASM module: {module_name}"))?;

        let mut store = Store::new(&engine, ());
        let fuel = self.effective_fuel(caps);
        if fuel > 0 {
            store.set_fuel(fuel).with_context(|| {
                format!("Failed to set fuel budget ({fuel}) for module: {module_name}")
            })?;
        }

        let linker = Linker::new(&engine);

        let instance = linker
            .instantiate(&mut store, &module)
            .and_then(|pre| pre.start(&mut store))
            .with_context(|| format!("Failed to instantiate WASM module: {module_name}"))?;

        let run_fn = instance
            .get_typed_func::<(), i32>(&store, "run")
            .or_else(|_| instance.get_typed_func::<(), i32>(&store, "_start"))
            .with_context(|| {
                format!(
                    "WASM module '{module_name}' must export a 'run() -> i32' or '_start() -> i32' function"
                )
            })?;

        let fuel_before = store.get_fuel().unwrap_or(0);
        let exit_code = match run_fn.call(&mut store, ()) {
            Ok(code) => code,
            Err(e) => {

                let fuel_after = store.get_fuel().unwrap_or(0);
                if fuel_after == 0 && fuel > 0 {
                    return Ok(WasmExecutionResult {
                        stdout: String::new(),
                        stderr: format!(
                            "WASM module '{module_name}' exceeded fuel limit ({fuel} ticks) — likely an infinite loop"
                        ),
                        exit_code: -1,
                        fuel_consumed: fuel,
                    });
                }
                bail!("WASM execution error in '{module_name}': {e}");
            }
        };
        let fuel_after = store.get_fuel().unwrap_or(0);
        let fuel_consumed = fuel_before.saturating_sub(fuel_after);

        Ok(WasmExecutionResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
            fuel_consumed,
        })
    }

    #[cfg(not(feature = "runtime-wasm"))]
    pub fn execute_module(
        &self,
        module_name: &str,
        _workspace_dir: &Path,
        _caps: &WasmCapabilities,
    ) -> Result<WasmExecutionResult> {
        bail!(
            "WASM runtime is not available in this build. \
             Rebuild with `cargo build --features runtime-wasm` to enable WASM sandbox support. \
             Module requested: {module_name}"
        )
    }

    pub fn list_modules(&self, workspace_dir: &Path) -> Result<Vec<String>> {
        let tools_path = self.tools_dir(workspace_dir);
        if !tools_path.exists() {
            return Ok(Vec::new());
        }

        let mut modules = Vec::new();
        for entry in std::fs::read_dir(&tools_path)
            .with_context(|| format!("Failed to read tools dir: {}", tools_path.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "wasm") {
                if let Some(stem) = path.file_stem() {
                    modules.push(stem.to_string_lossy().to_string());
                }
            }
        }
        modules.sort();
        Ok(modules)
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
             Use `execute_module()` to run WASM tools, or switch to runtime.kind = \"native\" for shell access."
        )
    }
}

