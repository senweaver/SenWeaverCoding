// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! macOS sandbox-exec (Seatbelt) sandbox backend.
//!
//! Uses Apple's built-in `sandbox-exec` tool to enforce per-session Seatbelt
//! profiles that restrict network access, filesystem writes, and process
//! spawning. Policy files are generated in `.sb` format and written to a
//! temporary directory that is cleaned up when the sandbox is dropped.

use crate::security::traits::Sandbox;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SeatbeltSandbox {

    policy_dir: PathBuf,

    policy_path: PathBuf,
}

impl SeatbeltSandbox {

    pub fn new() -> std::io::Result<Self> {
        if !Self::is_installed() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "sandbox-exec not found (requires macOS)",
            ));
        }

        let policy_dir = std::env::temp_dir().join("sen-seatbelt");
        std::fs::create_dir_all(&policy_dir)?;

        let session_id = uuid::Uuid::new_v4();
        let policy_path = policy_dir.join(format!("{session_id}.sb"));

        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"));
        let policy = generate_policy(&workspace);
        std::fs::write(&policy_path, &policy)?;

        Ok(Self {
            policy_dir,
            policy_path,
        })
    }

    pub fn probe() -> std::io::Result<Self> {
        Self::new()
    }

    fn is_installed() -> bool {

        Path::new("/usr/bin/sandbox-exec").exists()
            || crate::util::hidden_sync_command("sandbox-exec")
                .arg("-n")
                .arg("no-network")
                .arg("true")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
    }

    pub fn policy_path(&self) -> &Path {
        &self.policy_path
    }

    pub fn policy_dir(&self) -> &Path {
        &self.policy_dir
    }
}

impl Drop for SeatbeltSandbox {
    fn drop(&mut self) {

        let _ = std::fs::remove_file(&self.policy_path);
    }
}

impl Sandbox for SeatbeltSandbox {
    fn wrap_command(&self, cmd: &mut Command) -> std::io::Result<()> {
        let program = cmd.get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        let mut sandbox_cmd = crate::util::hidden_sync_command("sandbox-exec");
        sandbox_cmd.arg("-f");
        sandbox_cmd.arg(&self.policy_path);
        sandbox_cmd.arg(&program);
        sandbox_cmd.args(&args);

        *cmd = sandbox_cmd;
        Ok(())
    }

    fn is_available(&self) -> bool {
        Self::is_installed() && self.policy_path.exists()
    }

    fn name(&self) -> &str {
        "sandbox-exec"
    }

    fn description(&self) -> &str {
        "macOS Seatbelt sandbox (built-in sandbox-exec)"
    }
}

fn generate_policy(workspace: &Path) -> String {
    let workspace_str = workspace.to_string_lossy();

    format!(
        r#"(version 1)

;; Deny everything by default
(deny default)

;; ── Process execution ──────────────────────────────────────
;; Allow basic process operations needed for command execution
(allow process-exec)
(allow process-fork)
(allow signal (target self))

;; ── Filesystem reads ───────────────────────────────────────
;; Allow reading system libraries, frameworks, and executables
(allow file-read*
    (subpath "/usr")
    (subpath "/bin")
    (subpath "/sbin")
    (subpath "/Library")
    (subpath "/System")
    (subpath "/private/var")
    (subpath "/dev")
    (subpath "/etc")
    (subpath "/Applications")
    (subpath "/opt")
    (subpath "/nix")
    (literal "/")
    (subpath "/var"))

;; Allow reading the workspace
(allow file-read* (subpath "{workspace}"))

;; Allow reading temp directories (needed for policy file itself)
(allow file-read* (subpath "/tmp"))
(allow file-read* (subpath "/private/tmp"))
(allow file-read*
    (regex #"^/private/var/folders/"))

;; Allow reading user home for tool configs
(allow file-read*
    (regex #"^/Users/[^/]+/\\."))

;; ── Filesystem writes ──────────────────────────────────────
;; Only allow writes to workspace and temp directories
(allow file-write*
    (subpath "{workspace}"))
(allow file-write*
    (subpath "/tmp")
    (subpath "/private/tmp"))
(allow file-write*
    (regex #"^/private/var/folders/"))
(allow file-write* (subpath "/dev/null"))
(allow file-write* (subpath "/dev/tty"))

;; ── Network ────────────────────────────────────────────────
;; Deny all network by default (inherited from deny default)
;; Allow DNS resolution only
(allow network-outbound
    (remote unix-socket (path-literal "/var/run/mDNSResponder")))
(allow system-socket)

;; Allow localhost connections only (for local dev servers).
;; Note: macOS sandbox-exec only accepts "localhost:*" or "*:port" in
;; (remote ip ...) filters — raw IPs like "127.0.0.1:*" cause the
;; entire policy to fail to parse.
(allow network-outbound
    (remote ip "localhost:*"))

;; ── Mach / IPC ─────────────────────────────────────────────
;; Allow basic mach services needed for process execution
(allow mach-lookup
    (global-name "com.apple.system.logger")
    (global-name "com.apple.system.notification_center")
    (global-name "com.apple.SecurityServer")
    (global-name "com.apple.CoreServices.coreservicesd"))

;; ── Sysctl / misc ──────────────────────────────────────────
(allow sysctl-read)
(allow mach-task-name)
"#,
        workspace = workspace_str,
    )
}
