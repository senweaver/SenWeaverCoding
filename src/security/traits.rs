// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use std::process::Command;

#[async_trait]
pub trait Sandbox: Send + Sync {

    fn wrap_command(&self, cmd: &mut Command) -> std::io::Result<()>;

    fn is_available(&self) -> bool;

    fn name(&self) -> &str;

    fn description(&self) -> &str;
}

#[derive(Debug, Clone, Default)]
pub struct NoopSandbox;

impl Sandbox for NoopSandbox {
    fn wrap_command(&self, _cmd: &mut Command) -> std::io::Result<()> {

        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "none"
    }

    fn description(&self) -> &str {
        "No sandboxing (application-layer security only)"
    }
}

#[derive(Debug, Clone)]
pub struct UnavailableSandbox {
    requested: String,
}

impl UnavailableSandbox {
    pub fn new(requested: impl Into<String>) -> Self {
        Self {
            requested: requested.into(),
        }
    }

    pub fn requested_backend(&self) -> &str {
        &self.requested
    }
}

impl Sandbox for UnavailableSandbox {
    fn wrap_command(&self, _cmd: &mut Command) -> std::io::Result<()> {
        Err(std::io::Error::other(format!(
            "sandbox backend '{}' was explicitly requested in [security.sandbox].backend but is \
             not available on this system; refusing to run commands without the requested \
             isolation. Install/enable the backend, or set backend = \"auto\" or \"none\" to \
             proceed without it.",
            self.requested
        )))
    }

    fn is_available(&self) -> bool {
        false
    }

    fn name(&self) -> &str {
        "unavailable"
    }

    fn description(&self) -> &str {
        "Requested sandbox backend unavailable (fail-closed: command execution is blocked)"
    }
}
