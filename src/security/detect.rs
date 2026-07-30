// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::{SandboxBackend, SecurityConfig};
use crate::security::traits::Sandbox;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Clone)]
pub struct SandboxStatus {
    pub requested: String,
    pub effective: String,
    pub requested_but_failed: bool,
}

static SANDBOX_STATUS: OnceLock<RwLock<Option<SandboxStatus>>> = OnceLock::new();

fn status_cell() -> &'static RwLock<Option<SandboxStatus>> {
    SANDBOX_STATUS.get_or_init(|| RwLock::new(None))
}

pub fn active_sandbox_status() -> Option<SandboxStatus> {
    status_cell().read().ok().and_then(|guard| guard.clone())
}

pub fn ensure_sandbox_available() -> Result<(), String> {
    match active_sandbox_status() {
        Some(status) if status.requested_but_failed => Err(format!(
            "sandbox backend '{}' was explicitly requested in [security.sandbox].backend but is \
             not available on this system; refusing to execute without the requested isolation. \
             Install/enable the backend, or set backend = \"auto\" or \"none\" to opt out.",
            status.requested
        )),
        _ => Ok(()),
    }
}

fn record_status(requested: &str, effective: &str, requested_but_failed: bool) {
    if let Ok(mut guard) = status_cell().write() {
        *guard = Some(SandboxStatus {
            requested: requested.to_string(),
            effective: effective.to_string(),
            requested_but_failed,
        });
    }
}

fn backend_label(backend: &SandboxBackend) -> &'static str {
    match backend {
        SandboxBackend::Auto => "auto",
        SandboxBackend::Landlock => "landlock",
        SandboxBackend::Firejail => "firejail",
        SandboxBackend::Bubblewrap => "bubblewrap",
        SandboxBackend::Docker => "docker",
        SandboxBackend::SandboxExec => "sandbox-exec",
        SandboxBackend::Wasm => "wasm",
        SandboxBackend::None => "none",
    }
}

fn explicit_backend_failed(requested: &'static str) -> Arc<dyn Sandbox> {
    tracing::error!(
        backend = requested,
        "explicitly requested sandbox backend is unavailable; failing closed \
         (shell command execution will be blocked until the backend is available \
          or [security.sandbox].backend is changed to \"auto\"/\"none\")"
    );
    record_status(requested, "unavailable", true);
    Arc::new(super::traits::UnavailableSandbox::new(requested))
}

pub fn create_sandbox(config: &SecurityConfig, workspace: Option<&Path>) -> Arc<dyn Sandbox> {
    let backend = &config.sandbox.backend;
    let _workspace: Option<PathBuf> = workspace.map(Path::to_path_buf);

    if matches!(backend, SandboxBackend::None) || config.sandbox.enabled == Some(false) {
        record_status(backend_label(backend), "none", false);
        return Arc::new(super::traits::NoopSandbox);
    }

    match backend {
        SandboxBackend::Landlock => {
            #[cfg(feature = "sandbox-landlock")]
            {
                #[cfg(target_os = "linux")]
                {
                    if let Ok(sandbox) =
                        super::landlock::LandlockSandbox::with_workspace(_workspace.clone())
                    {
                        record_status("landlock", sandbox.name(), false);
                        return Arc::new(sandbox);
                    }
                }
            }
            explicit_backend_failed("landlock")
        }
        SandboxBackend::Firejail => {
            #[cfg(target_os = "linux")]
            {
                if let Ok(sandbox) = super::firejail::FirejailSandbox::new() {
                    record_status("firejail", sandbox.name(), false);
                    return Arc::new(sandbox);
                }
            }
            explicit_backend_failed("firejail")
        }
        SandboxBackend::Bubblewrap => {
            #[cfg(feature = "sandbox-bubblewrap")]
            {
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                {
                    if let Ok(sandbox) = super::bubblewrap::BubblewrapSandbox::new() {
                        record_status("bubblewrap", sandbox.name(), false);
                        return Arc::new(sandbox);
                    }
                }
            }
            explicit_backend_failed("bubblewrap")
        }
        SandboxBackend::Docker => {
            if let Ok(sandbox) = super::docker::DockerSandbox::new() {
                let sandbox = match _workspace.clone() {
                    Some(ws) => sandbox.with_workspace(ws),
                    None => sandbox,
                };
                record_status("docker", sandbox.name(), false);
                return Arc::new(sandbox);
            }
            explicit_backend_failed("docker")
        }
        SandboxBackend::SandboxExec => {
            #[cfg(target_os = "macos")]
            {
                if let Ok(sandbox) = super::seatbelt::SeatbeltSandbox::new() {
                    record_status("sandbox-exec", sandbox.name(), false);
                    return Arc::new(sandbox);
                }
            }
            explicit_backend_failed("sandbox-exec")
        }
        SandboxBackend::Wasm => {
            tracing::info!(
                "WASM sandbox requested  -  WASM isolation applies to runtime.kind='wasm' modules. \
                 Shell commands still use application-layer security."
            );
            record_status("wasm", "none", false);
            Arc::new(super::traits::NoopSandbox)
        }
        SandboxBackend::Auto | SandboxBackend::None => {
            let sandbox = detect_best_sandbox(_workspace);
            record_status(backend_label(backend), sandbox.name(), false);
            sandbox
        }
    }
}

fn detect_best_sandbox(workspace: Option<PathBuf>) -> Arc<dyn Sandbox> {
    let _ = &workspace;
    #[cfg(target_os = "linux")]
    {

        #[cfg(feature = "sandbox-landlock")]
        {
            if let Ok(sandbox) = super::landlock::LandlockSandbox::with_workspace(workspace.clone())
            {
                tracing::info!("Landlock sandbox enabled (Linux kernel 5.13+)");
                return Arc::new(sandbox);
            }
        }

        if let Ok(sandbox) = super::firejail::FirejailSandbox::probe() {
            tracing::info!("Firejail sandbox enabled");
            return Arc::new(sandbox);
        }
    }

    #[cfg(all(target_os = "windows", feature = "sandbox-windows-job"))]
    {
        if let Ok(sandbox) = super::job_object::JobObjectSandbox::probe() {
            tracing::info!(
                "Windows Job Object sandbox enabled (memory / CPU / process-count caps)"
            );
            return Arc::new(sandbox);
        }
    }

    #[cfg(target_os = "macos")]
    {

        #[cfg(feature = "sandbox-bubblewrap")]
        {
            if let Ok(sandbox) = super::bubblewrap::BubblewrapSandbox::probe() {
                tracing::info!("Bubblewrap sandbox enabled");
                return Arc::new(sandbox);
            }
        }

        if let Ok(sandbox) = super::seatbelt::SeatbeltSandbox::probe() {
            tracing::info!("macOS sandbox-exec (Seatbelt) enabled");
            return Arc::new(sandbox);
        }
    }

    if let Ok(sandbox) = super::docker::DockerSandbox::probe() {
        let sandbox = match workspace.clone() {
            Some(ws) => sandbox.with_workspace(ws),
            None => sandbox,
        };
        tracing::info!("Docker sandbox enabled");
        return Arc::new(sandbox);
    }

    tracing::info!("No sandbox backend available, using application-layer security");
    Arc::new(super::traits::NoopSandbox)
}
