// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::{SandboxBackend, SecurityConfig};
use crate::security::traits::Sandbox;
use std::sync::Arc;

pub fn create_sandbox(config: &SecurityConfig) -> Arc<dyn Sandbox> {
    let backend = &config.sandbox.backend;

    if matches!(backend, SandboxBackend::None) || config.sandbox.enabled == Some(false) {
        return Arc::new(super::traits::NoopSandbox);
    }

    match backend {
        SandboxBackend::Landlock => {
            #[cfg(feature = "sandbox-landlock")]
            {
                #[cfg(target_os = "linux")]
                {
                    if let Ok(sandbox) = super::landlock::LandlockSandbox::new() {
                        return Arc::new(sandbox);
                    }
                }
            }
            tracing::warn!(
                "Landlock requested but not available, falling back to application-layer"
            );
            Arc::new(super::traits::NoopSandbox)
        }
        SandboxBackend::Firejail => {
            #[cfg(target_os = "linux")]
            {
                if let Ok(sandbox) = super::firejail::FirejailSandbox::new() {
                    return Arc::new(sandbox);
                }
            }
            tracing::warn!(
                "Firejail requested but not available, falling back to application-layer"
            );
            Arc::new(super::traits::NoopSandbox)
        }
        SandboxBackend::Bubblewrap => {
            #[cfg(feature = "sandbox-bubblewrap")]
            {
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                {
                    if let Ok(sandbox) = super::bubblewrap::BubblewrapSandbox::new() {
                        return Arc::new(sandbox);
                    }
                }
            }
            tracing::warn!(
                "Bubblewrap requested but not available, falling back to application-layer"
            );
            Arc::new(super::traits::NoopSandbox)
        }
        SandboxBackend::Docker => {
            if let Ok(sandbox) = super::docker::DockerSandbox::new() {
                return Arc::new(sandbox);
            }
            tracing::warn!("Docker requested but not available, falling back to application-layer");
            Arc::new(super::traits::NoopSandbox)
        }
        SandboxBackend::SandboxExec => {
            #[cfg(target_os = "macos")]
            {
                if let Ok(sandbox) = super::seatbelt::SeatbeltSandbox::new() {
                    return Arc::new(sandbox);
                }
            }
            tracing::warn!(
                "sandbox-exec requested but not available, falling back to application-layer"
            );
            Arc::new(super::traits::NoopSandbox)
        }
        SandboxBackend::Wasm => {
            tracing::info!(
                "WASM sandbox requested — WASM isolation applies to runtime.kind='wasm' modules. \
                 Shell commands still use application-layer security."
            );
            Arc::new(super::traits::NoopSandbox)
        }
        SandboxBackend::Auto | SandboxBackend::None => {

            detect_best_sandbox()
        }
    }
}

fn detect_best_sandbox() -> Arc<dyn Sandbox> {
    #[cfg(target_os = "linux")]
    {

        #[cfg(feature = "sandbox-landlock")]
        {
            if let Ok(sandbox) = super::landlock::LandlockSandbox::probe() {
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
        tracing::info!("Docker sandbox enabled");
        return Arc::new(sandbox);
    }

    tracing::info!("No sandbox backend available, using application-layer security");
    Arc::new(super::traits::NoopSandbox)
}
