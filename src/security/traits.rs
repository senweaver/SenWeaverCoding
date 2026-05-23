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
