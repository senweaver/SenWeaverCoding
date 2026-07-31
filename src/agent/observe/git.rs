// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;
use std::process::Stdio;

pub async fn git_change_summary(cwd: &Path) -> Option<String> {
    let status = run_git(cwd, &["status", "--porcelain"]).await?;
    let status = status.trim();
    if status.is_empty() {
        return None;
    }

    let diff_stat = run_git(cwd, &["diff", "--stat"])
        .await
        .unwrap_or_default();

    let mut summary = String::from("Working tree changes:\n");
    for line in status.lines().take(80) {
        summary.push_str(line);
        summary.push('\n');
    }
    let diff_stat = diff_stat.trim();
    if !diff_stat.is_empty() {
        summary.push_str("\nDiff stat:\n");
        for line in diff_stat.lines().take(80) {
            summary.push_str(line);
            summary.push('\n');
        }
    }
    Some(summary)
}

async fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = crate::util::hidden_async_command("git");
    cmd.args(args);
    if !cwd.as_os_str().is_empty() {
        cmd.current_dir(cwd);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(crate::util::decode_subprocess_bytes(&output.stdout))
}
