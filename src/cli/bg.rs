// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Background session management — list, inspect, attach, and kill sessions.

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

/// Get the sessions directory
fn sessions_dir(workspace: &Path) -> PathBuf {
    workspace.join(".senweavercoding").join("sessions")
}

/// List all background sessions
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

/// Get logs for a specific session
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

/// Kill a background session by ID
pub async fn kill_session(workspace: &Path, session_id: &str) -> Result<()> {
    let session_file = sessions_dir(workspace).join(format!("{}.json", session_id));
    if !session_file.exists() {
        anyhow::bail!("Session '{}' not found", session_id);
    }

    let data = tokio::fs::read_to_string(&session_file).await?;
    let info: SessionInfo = serde_json::from_str(&data)?;

    if let Some(pid) = info.pid {
        #[cfg(unix)]
        {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        #[cfg(windows)]
        {
            let _ = tokio::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output()
                .await;
        }
        tracing::info!(
            "Sent termination signal to session '{}' (PID {})",
            session_id,
            pid
        );
    }

    // Update session status
    let updated = SessionInfo {
        status: SessionStatus::Stopped,
        ..info
    };
    tokio::fs::write(&session_file, serde_json::to_string_pretty(&updated)?).await?;

    Ok(())
}

/// Save session info to disk
pub async fn save_session(workspace: &Path, info: &SessionInfo) -> Result<()> {
    let dir = sessions_dir(workspace);
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{}.json", info.id));
    tokio::fs::write(&path, serde_json::to_string_pretty(info)?).await?;
    Ok(())
}

/// Synchronous version of `list_sessions` for use from non-async contexts
/// (e.g. the ratatui event loop). Uses blocking std::fs.
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

/// Print sessions in a table format
pub fn print_sessions(sessions: &[SessionInfo]) {
    if sessions.is_empty() {
        println!("No background sessions found.");
        return;
    }

    println!(
        "{:<36} {:<10} {:<24} {}",
        "SESSION ID", "STATUS", "STARTED", "CWD"
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_status_display() {
        assert_eq!(SessionStatus::Running.to_string(), "running");
        assert_eq!(SessionStatus::Stopped.to_string(), "stopped");
    }

    #[test]
    fn session_info_serde_roundtrip() {
        let info = SessionInfo {
            id: "test-123".into(),
            pid: Some(42),
            started_at: "2025-01-01T00:00:00Z".into(),
            status: SessionStatus::Running,
            cwd: PathBuf::from("/tmp"),
            last_activity: "2025-01-01T00:01:00Z".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: SessionInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-123");
        assert_eq!(parsed.status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn list_sessions_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sessions = list_sessions(tmp.path()).await.unwrap();
        assert!(sessions.is_empty());
    }
}
