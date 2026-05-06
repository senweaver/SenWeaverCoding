// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Landlock sandbox (Linux kernel 5.13+ LSM)
//!
//! Landlock provides unprivileged sandboxing through the Linux kernel.
//! This module uses the pure-Rust `landlock` crate for filesystem access control.

#[cfg(all(feature = "sandbox-landlock", target_os = "linux"))]
use landlock::{AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr};
#[cfg(all(feature = "sandbox-landlock", target_os = "linux"))]
use std::path::Path;

use crate::security::traits::Sandbox;

#[cfg(all(feature = "sandbox-landlock", target_os = "linux"))]
#[derive(Debug)]
pub struct LandlockSandbox {
    workspace_dir: Option<std::path::PathBuf>,
}

#[cfg(all(feature = "sandbox-landlock", target_os = "linux"))]
impl LandlockSandbox {

    pub fn new() -> std::io::Result<Self> {
        Self::with_workspace(None)
    }

    pub fn with_workspace(workspace_dir: Option<std::path::PathBuf>) -> std::io::Result<Self> {

        let test_ruleset = Ruleset::default()
            .handle_access(AccessFs::ReadFile | AccessFs::WriteFile)
            .and_then(|ruleset| ruleset.create());

        match test_ruleset {
            Ok(_) => Ok(Self { workspace_dir }),
            Err(e) => {
                tracing::debug!("Landlock not available: {}", e);
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Landlock not available",
                ))
            }
        }
    }

    pub fn probe() -> std::io::Result<Self> {
        Self::new()
    }

    fn apply_restrictions(&self) -> std::io::Result<()> {
        let mut ruleset = Ruleset::default()
            .handle_access(
                AccessFs::ReadFile
                    | AccessFs::WriteFile
                    | AccessFs::ReadDir
                    | AccessFs::RemoveDir
                    | AccessFs::RemoveFile
                    | AccessFs::MakeChar
                    | AccessFs::MakeSock
                    | AccessFs::MakeFifo
                    | AccessFs::MakeBlock
                    | AccessFs::MakeReg
                    | AccessFs::MakeSym,
            )
            .and_then(|ruleset| ruleset.create())
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        if let Some(ref workspace) = self.workspace_dir {
            if workspace.exists() {
                let workspace_fd =
                    PathFd::new(workspace).map_err(|e| std::io::Error::other(e.to_string()))?;
                ruleset = ruleset
                    .add_rule(PathBeneath::new(
                        workspace_fd,
                        AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::ReadDir,
                    ))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
        }

        let tmp_fd =
            PathFd::new(Path::new("/tmp")).map_err(|e| std::io::Error::other(e.to_string()))?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(
                tmp_fd,
                AccessFs::ReadFile | AccessFs::WriteFile,
            ))
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let usr_fd =
            PathFd::new(Path::new("/usr")).map_err(|e| std::io::Error::other(e.to_string()))?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(
                usr_fd,
                AccessFs::ReadFile | AccessFs::ReadDir,
            ))
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let bin_fd =
            PathFd::new(Path::new("/bin")).map_err(|e| std::io::Error::other(e.to_string()))?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(
                bin_fd,
                AccessFs::ReadFile | AccessFs::ReadDir,
            ))
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        match ruleset.restrict_self() {
            Ok(_) => {
                tracing::debug!("Landlock restrictions applied successfully");
                Ok(())
            }
            Err(e) => {
                tracing::warn!("Failed to apply Landlock restrictions: {}", e);
                Err(std::io::Error::other(e.to_string()))
            }
        }
    }
}

#[cfg(all(feature = "sandbox-landlock", target_os = "linux"))]
impl Sandbox for LandlockSandbox {
    fn wrap_command(&self, cmd: &mut std::process::Command) -> std::io::Result<()> {
        use std::os::unix::process::CommandExt;

        let workspace_dir = self.workspace_dir.clone();
        unsafe {
            cmd.pre_exec(move || {
                let mut ruleset = Ruleset::default()
                    .handle_access(
                        AccessFs::ReadFile
                            | AccessFs::WriteFile
                            | AccessFs::ReadDir
                            | AccessFs::RemoveDir
                            | AccessFs::RemoveFile
                            | AccessFs::MakeChar
                            | AccessFs::MakeSock
                            | AccessFs::MakeFifo
                            | AccessFs::MakeBlock
                            | AccessFs::MakeReg
                            | AccessFs::MakeSym,
                    )
                    .and_then(|ruleset| ruleset.create())
                    .map_err(|e| std::io::Error::other(e.to_string()))?;

                if let Some(ref workspace) = workspace_dir {
                    if workspace.exists() {
                        let workspace_fd = PathFd::new(workspace)
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                        ruleset = ruleset
                            .add_rule(PathBeneath::new(
                                workspace_fd,
                                AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::ReadDir,
                            ))
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                    }
                }

                let tmp_fd = PathFd::new(Path::new("/tmp"))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                ruleset = ruleset
                    .add_rule(PathBeneath::new(
                        tmp_fd,
                        AccessFs::ReadFile | AccessFs::WriteFile,
                    ))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;

                let usr_fd = PathFd::new(Path::new("/usr"))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                ruleset = ruleset
                    .add_rule(PathBeneath::new(
                        usr_fd,
                        AccessFs::ReadFile | AccessFs::ReadDir,
                    ))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;

                let bin_fd = PathFd::new(Path::new("/bin"))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                ruleset = ruleset
                    .add_rule(PathBeneath::new(
                        bin_fd,
                        AccessFs::ReadFile | AccessFs::ReadDir,
                    ))
                    .map_err(|e| std::io::Error::other(e.to_string()))?;

                ruleset
                    .restrict_self()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                Ok(())
            });
        }
        Ok(())
    }

    fn is_available(&self) -> bool {

        Ruleset::default()
            .handle_access(AccessFs::ReadFile)
            .and_then(|ruleset| ruleset.create())
            .is_ok()
    }

    fn name(&self) -> &str {
        "landlock"
    }

    fn description(&self) -> &str {
        "Linux kernel LSM sandboxing (filesystem access control)"
    }
}

#[cfg(not(all(feature = "sandbox-landlock", target_os = "linux")))]
pub struct LandlockSandbox;

#[cfg(not(all(feature = "sandbox-landlock", target_os = "linux")))]
impl LandlockSandbox {
    pub fn new() -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Landlock is only supported on Linux with the sandbox-landlock feature",
        ))
    }

    pub fn with_workspace(_workspace_dir: Option<std::path::PathBuf>) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Landlock is only supported on Linux",
        ))
    }

    pub fn probe() -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Landlock is only supported on Linux",
        ))
    }
}

#[cfg(not(all(feature = "sandbox-landlock", target_os = "linux")))]
impl Sandbox for LandlockSandbox {
    fn wrap_command(&self, _cmd: &mut std::process::Command) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Landlock is only supported on Linux",
        ))
    }

    fn is_available(&self) -> bool {
        false
    }

    fn name(&self) -> &str {
        "landlock"
    }

    fn description(&self) -> &str {
        "Linux kernel LSM sandboxing (not available on this platform)"
    }
}
