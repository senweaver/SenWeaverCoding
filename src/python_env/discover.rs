// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::util::hidden_async_command;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectMarkers {
    pub has_venv_dir: bool,
    pub has_pyproject: bool,
    pub has_requirements: bool,
    pub has_pipfile: bool,
    pub has_setup_py: bool,
    pub has_setup_cfg: bool,
    pub has_python_version_file: bool,
    pub has_uv_lock: bool,
}

impl ProjectMarkers {
    pub fn is_python_project(&self) -> bool {
        self.has_venv_dir
            || self.has_pyproject
            || self.has_requirements
            || self.has_pipfile
            || self.has_setup_py
            || self.has_setup_cfg
            || self.has_python_version_file
            || self.has_uv_lock
    }
}

pub fn detect_workspace_project(workspace: &Path) -> ProjectMarkers {
    let exists = |name: &str| workspace.join(name).exists();
    let exists_dir = |name: &str| workspace.join(name).is_dir();
    ProjectMarkers {
        has_venv_dir: exists_dir(".venv") || exists_dir("venv") || exists_dir(".env"),
        has_pyproject: exists("pyproject.toml"),
        has_requirements: exists("requirements.txt") || exists("requirements-dev.txt"),
        has_pipfile: exists("Pipfile") || exists("Pipfile.lock"),
        has_setup_py: exists("setup.py"),
        has_setup_cfg: exists("setup.cfg"),
        has_python_version_file: exists(".python-version"),
        has_uv_lock: exists("uv.lock"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStrategy {
    UvSync,
    UvPipEditable,
    PipEditable,
    UvPipRequirements,
    PipRequirements,
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallRecommendation {
    pub strategy: InstallStrategy,
    pub target: Option<String>,
    pub uv_available: bool,
}

pub fn recommend_install_strategy(workspace: &Path) -> InstallRecommendation {
    let uv = uv_available_sync();
    let exists = |name: &str| workspace.join(name).exists();
    if exists("uv.lock") && uv {
        return InstallRecommendation {
            strategy: InstallStrategy::UvSync,
            target: Some("uv.lock".into()),
            uv_available: true,
        };
    }
    if exists("pyproject.toml") {
        return InstallRecommendation {
            strategy: if uv {
                InstallStrategy::UvPipEditable
            } else {
                InstallStrategy::PipEditable
            },
            target: Some("pyproject.toml".into()),
            uv_available: uv,
        };
    }
    if exists("requirements.txt") {
        return InstallRecommendation {
            strategy: if uv {
                InstallStrategy::UvPipRequirements
            } else {
                InstallStrategy::PipRequirements
            },
            target: Some("requirements.txt".into()),
            uv_available: uv,
        };
    }
    if exists("requirements-dev.txt") {
        return InstallRecommendation {
            strategy: if uv {
                InstallStrategy::UvPipRequirements
            } else {
                InstallStrategy::PipRequirements
            },
            target: Some("requirements-dev.txt".into()),
            uv_available: uv,
        };
    }
    InstallRecommendation {
        strategy: InstallStrategy::None,
        target: None,
        uv_available: uv,
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RequiredPython {
    pub version: Option<String>,
    pub source: Option<String>,
}

pub fn read_required_python(workspace: &Path) -> RequiredPython {
    if let Some(v) = read_python_version_file(workspace) {
        return RequiredPython {
            version: Some(v),
            source: Some(".python-version".into()),
        };
    }
    if let Some(v) = read_pyproject_python(workspace) {
        return v;
    }
    RequiredPython::default()
}

fn read_python_version_file(workspace: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(workspace.join(".python-version")).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let candidate = line.split_whitespace().next().unwrap_or("").to_string();
        if !candidate.is_empty() {
            return Some(strip_python_prefix(&candidate));
        }
    }
    None
}

fn strip_python_prefix(value: &str) -> String {
    let v = value.trim();
    let v = v.strip_prefix("python").unwrap_or(v);
    let v = v.strip_prefix("Python").unwrap_or(v);
    let v = v.strip_prefix('-').unwrap_or(v);
    v.trim().to_string()
}

fn read_pyproject_python(workspace: &Path) -> Option<RequiredPython> {
    let raw = std::fs::read_to_string(workspace.join("pyproject.toml")).ok()?;
    let doc: toml::Value = toml::from_str(&raw).ok()?;
    if let Some(tool) = doc.get("tool") {
        if let Some(uv_t) = tool.get("uv") {
            if let Some(python) = uv_t.get("python").and_then(|v| v.as_str()) {
                return Some(RequiredPython {
                    version: Some(strip_python_prefix(python)),
                    source: Some("pyproject.toml [tool.uv.python]".into()),
                });
            }
        }
        if let Some(poetry) = tool.get("poetry") {
            if let Some(deps) = poetry.get("dependencies") {
                if let Some(py) = deps.get("python").and_then(|v| v.as_str()) {
                    if let Some(extracted) = extract_min_version(py) {
                        return Some(RequiredPython {
                            version: Some(extracted),
                            source: Some(
                                "pyproject.toml [tool.poetry.dependencies.python]".into(),
                            ),
                        });
                    }
                }
            }
        }
    }
    if let Some(project) = doc.get("project") {
        if let Some(req) = project.get("requires-python").and_then(|v| v.as_str()) {
            if let Some(extracted) = extract_min_version(req) {
                return Some(RequiredPython {
                    version: Some(extracted),
                    source: Some("pyproject.toml [project.requires-python]".into()),
                });
            }
        }
    }
    None
}

fn extract_min_version(spec: &str) -> Option<String> {
    let mut chars = spec.chars().peekable();
    let mut found: Option<String> = None;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut buf = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_ascii_digit() || ch == '.' {
                    buf.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            if !buf.is_empty() {
                found = Some(buf);
                break;
            }
        } else {
            chars.next();
        }
    }
    found
}

#[derive(Debug, Clone, Serialize)]
pub struct InterpreterInfo {
    pub path: PathBuf,
    pub version: Option<String>,
    pub source: String,
    pub is_venv: bool,
}

pub async fn discover_interpreters(workspace: &Path) -> Vec<InterpreterInfo> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out: Vec<InterpreterInfo> = Vec::new();

    let venv_candidates = [
        workspace.join(".venv"),
        workspace.join("venv"),
        workspace.join(".env"),
    ];
    for venv in venv_candidates {
        if let Some(bin) = venv_interpreter_path(&venv) {
            if let Ok(canon) = bin.canonicalize() {
                if seen.insert(canon.clone()) {
                    let version = query_version(&canon).await;
                    out.push(InterpreterInfo {
                        path: canon,
                        version,
                        source: "workspace-venv".to_string(),
                        is_venv: true,
                    });
                }
            } else if seen.insert(bin.clone()) {
                let version = query_version(&bin).await;
                out.push(InterpreterInfo {
                    path: bin,
                    version,
                    source: "workspace-venv".to_string(),
                    is_venv: true,
                });
            }
        }
    }

    for candidate in system_python_candidates().await {
        let resolved = candidate.canonicalize().unwrap_or_else(|_| candidate.clone());
        if seen.insert(resolved.clone()) {
            let version = query_version(&resolved).await;
            out.push(InterpreterInfo {
                path: resolved,
                version,
                source: "system".to_string(),
                is_venv: false,
            });
        }
    }

    for candidate in uv_managed_pythons().await {
        let resolved = candidate.canonicalize().unwrap_or_else(|_| candidate.clone());
        if seen.insert(resolved.clone()) {
            let version = query_version(&resolved).await;
            out.push(InterpreterInfo {
                path: resolved,
                version,
                source: "uv".to_string(),
                is_venv: false,
            });
        }
    }

    out
}

pub fn venv_interpreter_path(venv: &Path) -> Option<PathBuf> {
    if !venv.is_dir() {
        return None;
    }
    let candidates = if cfg!(windows) {
        vec![
            venv.join("Scripts").join("python.exe"),
            venv.join("Scripts").join("python3.exe"),
            venv.join("bin").join("python.exe"),
        ]
    } else {
        vec![
            venv.join("bin").join("python3"),
            venv.join("bin").join("python"),
        ]
    };
    candidates.into_iter().find(|p| p.is_file())
}

async fn system_python_candidates() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        let output = run_capture("py", &["-0p"], 4).await;
        if let Some(text) = output {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(p) = line
                    .split_whitespace()
                    .find(|tok| tok.ends_with(".exe") || tok.contains("python"))
                {
                    let path = PathBuf::from(p);
                    if path.is_file() {
                        found.push(path);
                    }
                }
            }
        }
        for prog in ["python", "python3"] {
            if let Some(path) = which_first(prog).await {
                found.push(path);
            }
        }
    } else {
        for prog in ["python3", "python"] {
            if let Some(path) = which_all(prog).await {
                for p in path {
                    found.push(p);
                }
            }
        }
    }
    found
}

async fn uv_managed_pythons() -> Vec<PathBuf> {
    let Some(_uv) = which_first("uv").await else {
        return Vec::new();
    };
    let output = run_capture("uv", &["python", "list", "--only-installed"], 4).await;
    let Some(text) = output else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        for token in line.split_whitespace() {
            if token.contains(std::path::MAIN_SEPARATOR)
                && (token.ends_with("python") || token.ends_with("python.exe"))
            {
                let path = PathBuf::from(token);
                if path.is_file() {
                    out.push(path);
                }
            }
        }
    }
    out
}

async fn which_first(prog: &str) -> Option<PathBuf> {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("where", vec![prog])
    } else {
        ("which", vec![prog])
    };
    let text = run_capture(cmd, &args, 3).await?;
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
}

async fn which_all(prog: &str) -> Option<Vec<PathBuf>> {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("where", vec![prog])
    } else {
        ("which", vec!["-a", prog])
    };
    let text = run_capture(cmd, &args, 3).await?;
    let lines: Vec<PathBuf> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

pub async fn query_version(interpreter: &Path) -> Option<String> {
    let path_str = interpreter.to_string_lossy().to_string();
    let output = tokio::time::timeout(
        Duration::from_secs(4),
        hidden_async_command(&path_str)
            .arg("--version")
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() && output.stdout.is_empty() && output.stderr.is_empty() {
        return None;
    }
    let combined = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .strip_prefix("Python ")
            .unwrap_or(trimmed)
            .to_string(),
    )
}

async fn run_capture(cmd: &str, args: &[&str], timeout_secs: u64) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        hidden_async_command(cmd).args(args).output(),
    )
    .await
    .ok()?
    .ok()?;
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        combined.push('\n');
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    if combined.trim().is_empty() {
        None
    } else {
        Some(combined)
    }
}

pub fn uv_available_sync() -> bool {
    let prog = "uv";
    let cmd_name = if cfg!(windows) { "where" } else { "which" };
    let mut cmd = crate::util::hidden_sync_command(cmd_name);
    cmd.arg(prog);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.status()
        .map(|s| s.success())
        .unwrap_or(false)
}
