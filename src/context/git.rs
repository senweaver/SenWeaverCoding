// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct GitContext {
    pub branch: String,
    pub default_branch: Option<String>,
    pub status_short: String,
    pub recent_log: String,
    pub is_dirty: bool,
}

impl GitContext {

    pub async fn gather(cwd: &Path) -> anyhow::Result<Self> {
        let is_git = run_git(cwd, &["rev-parse", "--is-inside-work-tree"])
            .await
            .map(|o| o.trim() == "true")
            .unwrap_or(false);

        if !is_git {
            anyhow::bail!("Not a git repository");
        }

        let (branch, default_branch, status, log) = tokio::join!(
            run_git(cwd, &["branch", "--show-current"]),
            run_git(cwd, &["config", "init.defaultBranch"]),
            run_git(cwd, &["--no-optional-locks", "status", "--short"]),
            run_git(cwd, &["--no-optional-locks", "log", "--oneline", "-n", "5"]),
        );

        let branch = branch.unwrap_or_default().trim().to_string();
        let status_short = status
            .unwrap_or_default()
            .trim()
            .chars()
            .take(2000)
            .collect::<String>();
        let is_dirty = !status_short.is_empty();

        Ok(Self {
            branch,
            default_branch: default_branch.ok().map(|s| s.trim().to_string()),
            status_short,
            recent_log: log.unwrap_or_default().trim().to_string(),
            is_dirty,
        })
    }

}

async fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = crate::util::hidden_async_command("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        anyhow::bail!("git command failed: {:?}", args);
    }
}
