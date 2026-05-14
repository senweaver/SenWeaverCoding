// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// LocalShellTask — spawns a background shell command.
// Mirrors claude-code-typescript-src`tasks/LocalShellTask/`.

use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;

use super::types::{Task, TaskHandle, TaskId, TaskState, TaskType, generate_task_id};

pub struct LocalShellSpawnInput {
    pub command: String,
    pub description: String,
    pub timeout_ms: Option<u64>,
    pub tool_use_id: Option<String>,
    pub agent_id: Option<String>,
    pub kind: ShellTaskKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellTaskKind {
    Bash,
    Monitor,
}

pub struct LocalShellTask;

impl LocalShellTask {

    pub async fn spawn(
        input: LocalShellSpawnInput,
        cwd: PathBuf,
    ) -> anyhow::Result<(TaskState, TaskHandle)> {
        let task_id = generate_task_id(TaskType::LocalBash);
        let mut state = TaskState::new(
            task_id.clone(),
            TaskType::LocalBash,
            input.description.clone(),
            input.tool_use_id.clone(),
        );
        state.mark_running();

        let (cancel_tx, cancel_rx) = watch::channel(false);

        let output_file = state.output_file.clone();
        if let Some(parent) = output_file.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let command = input.command.clone();
        let timeout_ms = input.timeout_ms;

        let session_ctx_for_task = crate::session::current_session_context();
        crate::runtime::spawn_supervised("tasks.local_shell.run", async move {
            let _shell_guard = if let Some(ctx) = session_ctx_for_task.as_ref() {
                if let Some(manager) = crate::session::global_workspace_resources() {
                    match manager
                        .acquire(
                            &ctx.workspace_key,
                            crate::session::ResourceKind::Shell,
                            &ctx.session_id,
                            &ctx.title,
                        )
                        .await
                    {
                        Ok(g) => Some(g),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let _ = run_shell_command(&command, &cwd, &output_file, cancel_rx, timeout_ms).await;
        });

        let handle = TaskHandle {
            task_id: task_id.clone(),
            cancel_tx: Some(cancel_tx),
            cleanup: None,
        };

        Ok((state, handle))
    }
}

async fn run_shell_command(
    command: &str,
    cwd: &PathBuf,
    output_file: &PathBuf,
    mut cancel_rx: watch::Receiver<bool>,
    timeout_ms: Option<u64>,
) -> anyhow::Result<i32> {
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let shell_arg = if cfg!(windows) { "/C" } else { "-c" };

    let mut child = crate::util::hidden_async_command(shell)
        .arg(shell_arg)
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_path = output_file.clone();

    let writer_handle =
        crate::runtime::task_manager::spawn_supervised("tasks.local_shell_io", async move {
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&out_path)
                .await
                .ok();

            if let Some(stdout) = stdout {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(ref mut f) = file {
                        use tokio::io::AsyncWriteExt;
                        let _ = f.write_all(format!("{line}\n").as_bytes()).await;
                    }
                }
            }
            if let Some(stderr) = stderr {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(ref mut f) = file {
                        use tokio::io::AsyncWriteExt;
                        let _ = f.write_all(format!("[stderr] {line}\n").as_bytes()).await;
                    }
                }
            }
        });

    let exit_code = tokio::select! {
        result = child.wait() => {
            result.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1)
        }
        _ = cancel_rx.changed() => {
            let _ = child.kill().await;
            -1
        }
        _ = async {
            if let Some(ms) = timeout_ms {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            let _ = child.kill().await;
            -1
        }
    };

    let _ = writer_handle.into_inner().await;
    Ok(exit_code)
}

#[async_trait::async_trait]
impl Task for LocalShellTask {
    fn name(&self) -> &str {
        "LocalShellTask"
    }

    fn task_type(&self) -> TaskType {
        TaskType::LocalBash
    }

    async fn kill(&self, _task_id: &TaskId) -> anyhow::Result<()> {

        Ok(())
    }
}
