// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub pid: Option<u32>,
    pub started_at: String,
    pub status: SessionStatus,
    pub cwd: PathBuf,
    pub last_activity: String,

    #[serde(default)]
    pub pid_start_time: Option<u64>,

    #[serde(default)]
    pub argv0_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    Stopped,
    Crashed,
    Unknown,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Crashed => write!(f, "crashed"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

fn sessions_dir(workspace: &Path) -> PathBuf {
    workspace.join(".senweavercoding").join("sessions")
}

pub async fn list_sessions(workspace: &Path) -> Result<Vec<SessionInfo>> {
    let dir = sessions_dir(workspace);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(data) = tokio::fs::read_to_string(&path).await {
                if let Ok(info) = serde_json::from_str::<SessionInfo>(&data) {
                    sessions.push(info);
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(sessions)
}

pub async fn get_session_logs(
    workspace: &Path,
    session_id: &str,
    tail: Option<usize>,
) -> Result<String> {
    let log_path = sessions_dir(workspace).join(format!("{}.log", session_id));
    if !log_path.exists() {
        anyhow::bail!("No logs found for session '{}'", session_id);
    }

    let content = tokio::fs::read_to_string(&log_path).await?;
    if let Some(n) = tail {
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(n);
        Ok(lines[start..].join("\n"))
    } else {
        Ok(content)
    }
}

pub async fn kill_session(workspace: &Path, session_id: &str) -> Result<()> {
    let session_file = sessions_dir(workspace).join(format!("{}.json", session_id));
    if !session_file.exists() {
        anyhow::bail!("Session '{}' not found", session_id);
    }

    let data = tokio::fs::read_to_string(&session_file).await?;
    let info: SessionInfo = serde_json::from_str(&data)?;

    if let Some(pid) = info.pid {
        if !verify_pid_is_sen_with_start(pid, info.pid_start_time) {
            tracing::warn!(
                pid,
                session_id,
                "PID does not match recorded sen process (possible PID reuse) — skipping signal"
            );
        } else {
            safe_terminate_pid(pid).await;
            tracing::info!(
                "Sent termination signal to session '{}' (PID {})",
                session_id,
                pid
            );
        }
    }

    let updated = SessionInfo {
        status: SessionStatus::Stopped,
        ..info
    };
    tokio::fs::write(&session_file, serde_json::to_string_pretty(&updated)?).await?;

    Ok(())
}

pub async fn save_session(workspace: &Path, info: &SessionInfo) -> Result<()> {
    let dir = sessions_dir(workspace);
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{}.json", info.id));
    tokio::fs::write(&path, serde_json::to_string_pretty(info)?).await?;
    Ok(())
}

pub fn list_sessions_sync(workspace: &Path) -> Result<Vec<SessionInfo>> {
    let dir = sessions_dir(workspace);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(info) = serde_json::from_str::<SessionInfo>(&data) {
                    sessions.push(info);
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(sessions)
}

pub fn print_sessions(sessions: &[SessionInfo]) {
    if sessions.is_empty() {
        println!("No background sessions found.");
        return;
    }

    println!("{:<36} {:<10} {:<24} CWD", "SESSION ID", "STATUS", "STARTED");
    println!("{}", "-".repeat(90));
    for s in sessions {
        println!(
            "{:<36} {:<10} {:<24} {}",
            s.id,
            s.status,
            &s.started_at[..std::cmp::min(24, s.started_at.len())],
            s.cwd.display()
        );
    }
}

pub fn verify_pid_is_sen_with_start(pid: u32, expected_start_time: Option<u64>) -> bool {
    #[cfg(target_os = "linux")]
    {

        let exe_link = format!("/proc/{pid}/exe");
        let name_ok = match std::fs::read_link(&exe_link) {
            Ok(exe) => {
                let name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("");
                name == "sen" || name.starts_with("sen.")
            }
            Err(_) => return false,
        };
        if !name_ok {
            return false;
        }

        if let Some(expected) = expected_start_time {
            if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {

                if let Some(rest) = stat.rsplit(')').next() {
                    let fields: Vec<&str> = rest.split_whitespace().collect();

                    if let Some(st) = fields.get(19).and_then(|s| s.parse::<u64>().ok()) {
                        return st.abs_diff(expected) < 200;
                    }
                }
                return false;
            }
            return false;
        }
        true
    }
    #[cfg(target_os = "macos")]
    {

        let output = crate::util::hidden_sync_command("ps")
            .args(["-p", &pid.to_string(), "-o", "comm=,lstart="])
            .output();
        let Ok(out) = output else { return false };
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.trim();
        if line.is_empty() {
            return false;
        }
        let name_ok = line
            .split_whitespace()
            .next()
            .map(|s| s.ends_with("/sen") || s == "sen")
            .unwrap_or(false);
        if !name_ok {
            return false;
        }

        let _ = expected_start_time;
        true
    }
    #[cfg(target_os = "windows")]
    {
        let output = crate::util::hidden_sync_command("wmic")
            .args([
                "process",
                "where",
                &format!("ProcessId={pid}"),
                "get",
                "Name,CreationDate",
                "/value",
            ])
            .output();
        let Ok(out) = output else { return false };
        let text = String::from_utf8_lossy(&out.stdout);
        let name_ok = text.contains("sen.exe");
        if !name_ok {
            return false;
        }
        let _ = expected_start_time;
        true
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (pid, expected_start_time);
        false
    }
}

pub fn verify_pid_is_sen(pid: u32) -> bool {
    verify_pid_is_sen_with_start(pid, None)
}

#[cfg(target_os = "linux")]
pub fn capture_current_start_time() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = stat.rsplit(')').next()?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    fields.get(19)?.parse::<u64>().ok()
}

#[cfg(not(target_os = "linux"))]
pub fn capture_current_start_time() -> Option<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

pub async fn safe_terminate_pid(pid: u32) {
    #[cfg(unix)]
    {
        #[allow(unsafe_code)]
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
    #[cfg(windows)]
    {
        let _ = crate::util::hidden_async_command("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .await;
    }
}
