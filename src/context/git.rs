// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

const GIT_CONTEXT_TTL: std::time::Duration = std::time::Duration::from_secs(10);
const NOT_GIT_RECHECK_TTL: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct GitContext {
    pub branch: String,
    pub default_branch: Option<String>,
    pub status_short: String,
    pub recent_log: String,
    pub is_dirty: bool,
}

enum CachedGitContext {
    Context(std::time::Instant, GitContext),
    NotGit(std::time::Instant),
}

fn git_context_cache()
-> &'static parking_lot::Mutex<std::collections::HashMap<PathBuf, CachedGitContext>> {
    static CACHE: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<PathBuf, CachedGitContext>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

impl GitContext {

    pub async fn gather(cwd: &Path) -> anyhow::Result<Self> {
        {
            let cache = git_context_cache().lock();
            match cache.get(cwd) {
                Some(CachedGitContext::Context(cached_at, ctx))
                    if cached_at.elapsed() < GIT_CONTEXT_TTL =>
                {
                    return Ok(ctx.clone());
                }
                Some(CachedGitContext::NotGit(cached_at))
                    if cached_at.elapsed() < NOT_GIT_RECHECK_TTL =>
                {
                    anyhow::bail!("Not a git repository");
                }
                _ => {}
            }
        }

        let is_git = run_git(cwd, &["rev-parse", "--is-inside-work-tree"])
            .await
            .map(|o| o.trim() == "true")
            .unwrap_or(false);

        if !is_git {
            git_context_cache().lock().insert(
                cwd.to_path_buf(),
                CachedGitContext::NotGit(std::time::Instant::now()),
            );
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

        let ctx = Self {
            branch,
            default_branch: default_branch.ok().map(|s| s.trim().to_string()),
            status_short,
            recent_log: log.unwrap_or_default().trim().to_string(),
            is_dirty,
        };
        git_context_cache().lock().insert(
            cwd.to_path_buf(),
            CachedGitContext::Context(std::time::Instant::now(), ctx.clone()),
        );
        Ok(ctx)
    }

}

async fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = crate::util::hidden_async_command("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await?;
    if output.status.success() {
        Ok(crate::util::decode_subprocess_bytes(&output.stdout))
    } else {
        anyhow::bail!("git command failed: {:?}", args);
    }
}
