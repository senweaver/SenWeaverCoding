// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use super::cache::{forget_state, load_state, store_state};
use super::discover::{
    detect_workspace_project, query_version, read_required_python, recommend_install_strategy,
    uv_available_sync, venv_interpreter_path, InstallStrategy,
};
use super::events::{publish, PythonEnvEvent};
use crate::util::hidden_async_command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PythonInterpreterTool {
    Uv,
    Venv,
    System,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CreateTool {
    Uv,
    Venv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonEnvState {
    pub workspace: PathBuf,
    pub interpreter_path: Option<PathBuf>,
    pub version: Option<String>,
    pub tool: PythonInterpreterTool,
    pub is_isolated: bool,
    pub packages_count: Option<u32>,
    pub last_updated_ms: i64,
    pub last_error: Option<String>,
    #[serde(default)]
    pub is_python_project: bool,
}

impl PythonEnvState {
    pub fn empty(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            interpreter_path: None,
            version: None,
            tool: PythonInterpreterTool::Unknown,
            is_isolated: false,
            packages_count: None,
            last_updated_ms: now_ms(),
            last_error: None,
            is_python_project: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateOutcome {
    pub state: PythonEnvState,
    pub fallback_used: bool,
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn status_for(workspace: &Path) -> PythonEnvState {
    let cached = load_state(workspace).unwrap_or_else(|| PythonEnvState::empty(workspace));
    let markers = detect_workspace_project(workspace);
    PythonEnvState {
        is_python_project: markers.is_python_project(),
        ..cached
    }
}

pub async fn refresh_status(workspace: &Path) -> PythonEnvState {
    let mut state = status_for(workspace);

    if let Some(venv_python) = venv_interpreter_path(&workspace.join(".venv")) {
        let version = query_version(&venv_python).await;
        state.interpreter_path = Some(venv_python.clone());
        state.version = version;
        state.tool = if state.tool == PythonInterpreterTool::Uv {
            PythonInterpreterTool::Uv
        } else {
            PythonInterpreterTool::Venv
        };
        state.is_isolated = true;
        state.last_updated_ms = now_ms();
        state.last_error = None;
        store_state(workspace, &state);
        spawn_count_packages(workspace.to_path_buf(), venv_python);
        return state;
    }

    if let Some(path) = state.interpreter_path.clone() {
        if path.is_file() {
            let version = query_version(&path).await;
            state.version = version;
            state.last_updated_ms = now_ms();
            state.last_error = None;
            store_state(workspace, &state);
            spawn_count_packages(workspace.to_path_buf(), path);
            return state;
        }
    }

    state.interpreter_path = None;
    state.version = None;
    state.tool = PythonInterpreterTool::Unknown;
    state.is_isolated = false;
    state.packages_count = None;
    state.last_updated_ms = now_ms();
    store_state(workspace, &state);
    state
}

fn spawn_count_packages(workspace: PathBuf, interpreter: PathBuf) {
    tokio::spawn(async move {
        let Some(count) = count_packages(&interpreter).await else {
            return;
        };
        let Some(mut state) = load_state(&workspace) else {
            return;
        };
        if state
            .interpreter_path
            .as_ref()
            .map(|p| p == &interpreter)
            .unwrap_or(false)
        {
            state.packages_count = Some(count);
            state.last_updated_ms = now_ms();
            store_state(&workspace, &state);
            publish(PythonEnvEvent::PackagesCounted {
                workspace,
                count,
            });
        }
    });
}

pub async fn count_packages(interpreter: &Path) -> Option<u32> {
    let output = tokio::time::timeout(
        Duration::from_secs(6),
        hidden_async_command(interpreter.as_os_str())
            .args(["-m", "pip", "list", "--format=freeze"])
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let count = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .count();
    Some(count as u32)
}

pub async fn create_venv(
    workspace: &Path,
    requested: Option<CreateTool>,
    python_version: Option<&str>,
) -> Result<CreateOutcome, String> {
    let chosen = match requested {
        Some(CreateTool::Uv) => CreateTool::Uv,
        Some(CreateTool::Venv) => CreateTool::Venv,
        None => {
            if uv_available_sync() {
                CreateTool::Uv
            } else {
                CreateTool::Venv
            }
        }
    };
    let resolved_version = python_version
        .map(str::to_string)
        .or_else(|| read_required_python(workspace).version);
    publish(PythonEnvEvent::Creating {
        workspace: workspace.to_path_buf(),
        tool: match chosen {
            CreateTool::Uv => "uv".into(),
            CreateTool::Venv => "venv".into(),
        },
    });

    let mut fallback_used = false;
    let py_version_ref = resolved_version.as_deref();

    let run_result = match chosen {
        CreateTool::Uv => run_uv_venv(workspace, py_version_ref).await,
        CreateTool::Venv => run_python_venv(workspace, py_version_ref).await,
    };

    let final_result = match run_result {
        Ok(_) => Ok(()),
        Err(err) if matches!(chosen, CreateTool::Uv) => {
            tracing::warn!(error = %err, "uv venv failed, falling back to python -m venv");
            publish(PythonEnvEvent::Progress {
                workspace: workspace.to_path_buf(),
                message: format!("uv failed: {err}; falling back to python -m venv"),
            });
            fallback_used = true;
            run_python_venv(workspace, py_version_ref).await
        }
        Err(err) => Err(err),
    };

    if let Err(err) = final_result {
        publish(PythonEnvEvent::Failed {
            workspace: workspace.to_path_buf(),
            error: err.clone(),
        });
        let mut state = status_for(workspace);
        state.last_error = Some(err.clone());
        state.last_updated_ms = now_ms();
        store_state(workspace, &state);
        return Err(err);
    }

    if let Err(err) = append_gitignore(workspace) {
        tracing::warn!(error = %err, "failed to update .gitignore");
    }

    let state = refresh_status(workspace).await;
    publish(PythonEnvEvent::Ready {
        workspace: workspace.to_path_buf(),
        interpreter: state.interpreter_path.clone().unwrap_or_default(),
        version: state.version.clone(),
        fallback_used,
    });

    Ok(CreateOutcome {
        state,
        fallback_used,
    })
}

async fn run_uv_venv(workspace: &Path, python_version: Option<&str>) -> Result<(), String> {
    let mut cmd = hidden_async_command("uv");
    cmd.arg("venv").arg(".venv");
    if let Some(v) = python_version {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            cmd.arg("--python").arg(trimmed);
        }
    }
    cmd.current_dir(workspace);
    stream_to_events(workspace, cmd, "uv").await
}

async fn run_python_venv(workspace: &Path, python_version: Option<&str>) -> Result<(), String> {
    let python_bin = resolve_system_python(python_version).await?;
    let mut cmd = hidden_async_command(python_bin.as_os_str());
    cmd.arg("-m").arg("venv").arg(".venv");
    cmd.current_dir(workspace);
    stream_to_events(workspace, cmd, "venv").await
}

async fn resolve_system_python(python_version: Option<&str>) -> Result<PathBuf, String> {
    if cfg!(windows) {
        if let Some(v) = python_version {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                let arg = if trimmed.starts_with('-') {
                    trimmed.to_string()
                } else {
                    format!("-{trimmed}")
                };
                let attempt = tokio::time::timeout(
                    Duration::from_secs(4),
                    hidden_async_command("py").arg(&arg).arg("-c").arg("import sys;print(sys.executable)").output(),
                )
                .await;
                if let Ok(Ok(o)) = attempt {
                    if o.status.success() {
                        let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if !text.is_empty() {
                            return Ok(PathBuf::from(text));
                        }
                    }
                }
            }
        }
        for prog in ["py", "python", "python3"] {
            if let Some(path) = which_first(prog).await {
                return Ok(path);
            }
        }
    } else {
        let preferred: Vec<String> = if let Some(v) = python_version {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                vec!["python3".to_string(), "python".to_string()]
            } else {
                vec![
                    format!("python{trimmed}"),
                    "python3".to_string(),
                    "python".to_string(),
                ]
            }
        } else {
            vec!["python3".to_string(), "python".to_string()]
        };
        for prog in preferred {
            if let Some(path) = which_first(&prog).await {
                return Ok(path);
            }
        }
    }
    Err("No Python interpreter found on PATH. Please install Python 3.x.".to_string())
}

async fn which_first(prog: &str) -> Option<PathBuf> {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("where", vec![prog])
    } else {
        ("which", vec![prog])
    };
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        hidden_async_command(cmd).args(&args).output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(PathBuf::from)
}

async fn stream_to_events(workspace: &Path, mut cmd: Command, tool: &str) -> Result<(), String> {
    use std::process::Stdio;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {tool}: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let ws_owned = workspace.to_path_buf();

    let stdout_task = tokio::spawn({
        let ws_clone = ws_owned.clone();
        async move {
            if let Some(out) = stdout {
                let mut reader = tokio::io::BufReader::new(out).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    publish(PythonEnvEvent::Progress {
                        workspace: ws_clone.clone(),
                        message: line,
                    });
                }
            }
        }
    });
    let stderr_task = tokio::spawn({
        let ws_clone = ws_owned.clone();
        async move {
            if let Some(err) = stderr {
                let mut reader = tokio::io::BufReader::new(err).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    publish(PythonEnvEvent::Progress {
                        workspace: ws_clone.clone(),
                        message: line,
                    });
                }
            }
        }
    });
    let status = child
        .wait()
        .await
        .map_err(|e| format!("failed to await {tool}: {e}"))?;
    let _ = tokio::join!(stdout_task, stderr_task);
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{tool} exited with status {}",
            status.code().unwrap_or(-1)
        ))
    }
}

pub fn select_interpreter(
    workspace: &Path,
    interpreter_path: &Path,
) -> Result<PythonEnvState, String> {
    if !interpreter_path.is_file() {
        return Err(format!(
            "interpreter not found: {}",
            interpreter_path.display()
        ));
    }
    let canon = interpreter_path
        .canonicalize()
        .unwrap_or_else(|_| interpreter_path.to_path_buf());
    let venv_root = workspace.join(".venv");
    let canon_venv = venv_root.canonicalize().unwrap_or(venv_root);
    let is_in_workspace_venv = crate::util::path_is_within(&canon, &canon_venv);
    let mut state = status_for(workspace);
    state.interpreter_path = Some(canon.clone());
    state.is_isolated = is_in_workspace_venv;
    state.tool = if is_in_workspace_venv {
        if matches!(state.tool, PythonInterpreterTool::Uv) {
            PythonInterpreterTool::Uv
        } else {
            PythonInterpreterTool::Venv
        }
    } else {
        PythonInterpreterTool::System
    };
    state.last_updated_ms = now_ms();
    state.last_error = None;
    store_state(workspace, &state);
    Ok(state)
}

pub fn heal_missing_interpreter(workspace: &Path) {
    let Some(state) = load_state(workspace) else {
        return;
    };
    let Some(path) = state.interpreter_path.as_ref() else {
        return;
    };
    if path.is_file() {
        return;
    }
    let venv_python = venv_interpreter_path(&workspace.join(".venv"));
    let mut new_state = state.clone();
    new_state.last_updated_ms = now_ms();
    if let Some(vp) = venv_python {
        new_state.interpreter_path = Some(vp);
        new_state.is_isolated = true;
        new_state.last_error = None;
    } else {
        new_state.interpreter_path = None;
        new_state.is_isolated = false;
        new_state.tool = PythonInterpreterTool::Unknown;
        new_state.version = None;
        new_state.packages_count = None;
        new_state.last_error = Some("interpreter file disappeared".to_string());
    }
    store_state(workspace, &new_state);
}

pub fn purge_venv(workspace: &Path) -> Result<(), String> {
    let venv = workspace.join(".venv");
    if venv.is_dir() {
        std::fs::remove_dir_all(&venv).map_err(|e| format!("failed to delete .venv: {e}"))?;
    }
    forget_state(workspace);
    publish(PythonEnvEvent::Purged {
        workspace: workspace.to_path_buf(),
    });
    Ok(())
}

pub async fn install_requirements(
    workspace: &Path,
    file: Option<&str>,
) -> Result<String, String> {
    let req_name = file.unwrap_or("requirements.txt");
    let req_path = workspace.join(req_name);
    if !req_path.is_file() {
        return Err(format!("requirements file not found: {}", req_path.display()));
    }
    publish(PythonEnvEvent::InstallStart {
        workspace: workspace.to_path_buf(),
        file: req_path.clone(),
    });

    let state = load_state(workspace).unwrap_or_else(|| PythonEnvState::empty(workspace));
    let use_uv = matches!(state.tool, PythonInterpreterTool::Uv) || uv_available_sync();

    let outcome = if use_uv {
        run_uv_install(workspace, &req_path).await
    } else {
        run_pip_install(workspace, &state, &req_path).await
    };

    publish_install_done(workspace, &outcome);
    outcome
}

pub async fn install_with_strategy(workspace: &Path) -> Result<String, String> {
    let rec = recommend_install_strategy(workspace);
    let target_label = rec.target.clone().unwrap_or_else(|| "<auto>".to_string());
    publish(PythonEnvEvent::InstallStart {
        workspace: workspace.to_path_buf(),
        file: workspace.join(rec.target.clone().unwrap_or_default()),
    });
    let state = load_state(workspace).unwrap_or_else(|| PythonEnvState::empty(workspace));
    let outcome = match rec.strategy {
        InstallStrategy::UvSync => run_uv_sync(workspace).await,
        InstallStrategy::UvPipEditable => run_uv_pip_editable(workspace).await,
        InstallStrategy::PipEditable => run_pip_editable(workspace, &state).await,
        InstallStrategy::UvPipRequirements => {
            let path = workspace.join(target_label.clone());
            run_uv_install(workspace, &path).await
        }
        InstallStrategy::PipRequirements => {
            let path = workspace.join(target_label.clone());
            run_pip_install(workspace, &state, &path).await
        }
        InstallStrategy::None => Err("No installable manifest detected (requires uv.lock / pyproject.toml / requirements.txt)".to_string()),
    };
    publish_install_done(workspace, &outcome);
    outcome
}

fn publish_install_done(workspace: &Path, outcome: &Result<String, String>) {
    match outcome {
        Ok(_) => publish(PythonEnvEvent::InstallDone {
            workspace: workspace.to_path_buf(),
            success: true,
            message: None,
        }),
        Err(err) => publish(PythonEnvEvent::InstallDone {
            workspace: workspace.to_path_buf(),
            success: false,
            message: Some(err.clone()),
        }),
    }
}

async fn run_uv_sync(workspace: &Path) -> Result<String, String> {
    let mut cmd = hidden_async_command("uv");
    cmd.arg("sync").current_dir(workspace);
    let venv_dir = workspace.join(".venv");
    if venv_dir.is_dir() {
        cmd.env("VIRTUAL_ENV", &venv_dir);
        cmd.env("UV_PROJECT_ENVIRONMENT", &venv_dir);
    }
    stream_install(workspace, cmd, "uv sync").await
}

async fn run_uv_pip_editable(workspace: &Path) -> Result<String, String> {
    let mut cmd = hidden_async_command("uv");
    cmd.arg("pip").arg("install").arg("-e").arg(".");
    cmd.current_dir(workspace);
    let venv_dir = workspace.join(".venv");
    if venv_dir.is_dir() {
        cmd.env("VIRTUAL_ENV", &venv_dir);
        cmd.env("UV_PROJECT_ENVIRONMENT", &venv_dir);
    }
    stream_install(workspace, cmd, "uv pip install -e .").await
}

async fn run_pip_editable(
    workspace: &Path,
    state: &PythonEnvState,
) -> Result<String, String> {
    let interpreter = state
        .interpreter_path
        .clone()
        .or_else(|| venv_interpreter_path(&workspace.join(".venv")))
        .ok_or_else(|| "No interpreter available; create a venv first".to_string())?;
    let mut cmd = hidden_async_command(interpreter.as_os_str());
    cmd.arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-e")
        .arg(".")
        .current_dir(workspace);
    stream_install(workspace, cmd, "pip install -e .").await
}

async fn run_uv_install(workspace: &Path, req: &Path) -> Result<String, String> {
    let mut cmd = hidden_async_command("uv");
    cmd.arg("pip")
        .arg("install")
        .arg("-r")
        .arg(req)
        .current_dir(workspace);
    let venv_dir = workspace.join(".venv");
    if venv_dir.is_dir() {
        cmd.env("VIRTUAL_ENV", &venv_dir);
        cmd.env("UV_PROJECT_ENVIRONMENT", &venv_dir);
    }
    stream_install(workspace, cmd, "uv pip install").await
}

async fn run_pip_install(
    workspace: &Path,
    state: &PythonEnvState,
    req: &Path,
) -> Result<String, String> {
    let interpreter = state
        .interpreter_path
        .clone()
        .or_else(|| venv_interpreter_path(&workspace.join(".venv")))
        .ok_or_else(|| "No interpreter available; create a venv first".to_string())?;
    let mut cmd = hidden_async_command(interpreter.as_os_str());
    cmd.arg("-m").arg("pip").arg("install").arg("-r").arg(req);
    cmd.current_dir(workspace);
    stream_install(workspace, cmd, "pip install").await
}

async fn stream_install(workspace: &Path, mut cmd: Command, tool: &str) -> Result<String, String> {
    use std::process::Stdio;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {tool}: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let ws = workspace.to_path_buf();

    let stdout_task = tokio::spawn({
        let ws_clone = ws.clone();
        async move {
            if let Some(out) = stdout {
                let mut reader = tokio::io::BufReader::new(out).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    publish(PythonEnvEvent::InstallProgress {
                        workspace: ws_clone.clone(),
                        line,
                    });
                }
            }
        }
    });
    let stderr_task = tokio::spawn({
        let ws_clone = ws.clone();
        async move {
            if let Some(err) = stderr {
                let mut reader = tokio::io::BufReader::new(err).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    publish(PythonEnvEvent::InstallProgress {
                        workspace: ws_clone.clone(),
                        line,
                    });
                }
            }
        }
    });
    let status = child
        .wait()
        .await
        .map_err(|e| format!("failed to await {tool}: {e}"))?;
    let _ = tokio::join!(stdout_task, stderr_task);
    if status.success() {
        Ok(format!("{tool} completed"))
    } else {
        Err(format!("{tool} failed: exit {}", status.code().unwrap_or(-1)))
    }
}

fn append_gitignore(workspace: &Path) -> std::io::Result<()> {
    let gi = workspace.join(".gitignore");
    let existing = std::fs::read_to_string(&gi).unwrap_or_default();
    let needs_venv = !existing.lines().any(|l| l.trim() == ".venv" || l.trim() == ".venv/");
    if !needs_venv {
        return Ok(());
    }
    let mut new_content = existing.clone();
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    if !new_content.contains("# Python virtual environment") {
        new_content.push_str("# Python virtual environment\n");
    }
    new_content.push_str(".venv/\n");
    std::fs::write(gi, new_content)?;
    Ok(())
}
