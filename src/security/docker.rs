// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::security::traits::Sandbox;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DockerSandbox {
    image: String,
    workspace_dir: Option<std::path::PathBuf>,
}

impl Default for DockerSandbox {
    fn default() -> Self {
        Self {
            image: "alpine:latest".to_string(),
            workspace_dir: None,
        }
    }
}

impl DockerSandbox {
    pub fn new() -> std::io::Result<Self> {
        if Self::is_installed() {
            Ok(Self::default())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Docker not found",
            ))
        }
    }

    pub fn with_image(image: String) -> std::io::Result<Self> {
        if Self::is_installed() {
            Ok(Self {
                image,
                workspace_dir: None,
            })
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Docker not found",
            ))
        }
    }

    pub fn with_workspace(mut self, workspace_dir: std::path::PathBuf) -> Self {
        self.workspace_dir = Some(workspace_dir);
        self
    }

    pub fn probe() -> std::io::Result<Self> {
        Self::new()
    }

    fn is_installed() -> bool {
        crate::util::hidden_sync_command("docker")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Sandbox for DockerSandbox {
    fn wrap_command(&self, cmd: &mut Command) -> std::io::Result<()> {
        let program = cmd.get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        let mut docker_cmd = crate::util::hidden_sync_command("docker");
        docker_cmd.args([
            "run",
            "--rm",
            "--memory",
            "512m",
            "--cpus",
            "1.0",
            "--network",
            "none",
        ]);

        if let Some(ref workspace) = self.workspace_dir {
            let ws = workspace.display().to_string();
            docker_cmd.args(["-v", &format!("{ws}:{ws}:ro"), "--workdir", &ws]);
        }

        docker_cmd.arg(&self.image);
        docker_cmd.arg(&program);
        docker_cmd.args(&args);

        *cmd = docker_cmd;
        Ok(())
    }

    fn is_available(&self) -> bool {
        Self::is_installed()
    }

    fn name(&self) -> &str {
        "docker"
    }

    fn description(&self) -> &str {
        "Docker container isolation (requires docker)"
    }
}
