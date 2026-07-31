// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[allow(dead_code)]
static DEGRADED_WARNED: OnceLock<()> = OnceLock::new();

struct FsConfinement {
    enabled: bool,
    workspace: Option<PathBuf>,
    allowed_roots: Vec<PathBuf>,
    session_workspaces: std::collections::HashMap<String, PathBuf>,
}

const MAX_SESSION_WORKSPACES: usize = 256;

static FS_CONFINEMENT: OnceLock<RwLock<FsConfinement>> = OnceLock::new();

fn confinement() -> &'static RwLock<FsConfinement> {
    FS_CONFINEMENT.get_or_init(|| {
        RwLock::new(FsConfinement {
            enabled: false,
            workspace: None,
            allowed_roots: Vec::new(),
            session_workspaces: std::collections::HashMap::new(),
        })
    })
}

pub fn configure_fs_confinement(
    enabled: bool,
    workspace: Option<PathBuf>,
    allowed_roots: Vec<PathBuf>,
) {
    let mut guard = confinement().write();
    guard.enabled = enabled;
    if let Some(ws) = workspace {
        if !ws.as_os_str().is_empty() {
            guard.workspace = Some(resolve_real_path(&ws));
        }
    }
    guard.allowed_roots = allowed_roots
        .into_iter()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| resolve_real_path(&p))
        .collect();
}

pub fn register_workspace_root(path: &Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    let normalized = resolve_real_path(path);
    let session_id = crate::session::current_session_context().map(|c| c.session_id);
    let mut guard = confinement().write();
    match session_id {
        Some(id) => {
            insert_session_workspace(&mut guard, id, normalized.clone());
            if guard.workspace.is_none() {
                guard.workspace = Some(normalized);
            }
        }
        None => {
            if guard.session_workspaces.is_empty() || guard.workspace.is_none() {
                guard.workspace = Some(normalized);
            }
        }
    }
}

fn insert_session_workspace(guard: &mut FsConfinement, id: String, root: PathBuf) {
    if guard.session_workspaces.len() >= MAX_SESSION_WORKSPACES
        && !guard.session_workspaces.contains_key(&id)
    {
        let victim = guard
            .session_workspaces
            .keys()
            .find(|k| crate::session::is_session_running_global(k) == Some(false))
            .or_else(|| {
                guard
                    .session_workspaces
                    .keys()
                    .find(|k| crate::session::is_session_running_global(k).is_none())
            })
            .cloned();
        match victim {
            Some(victim) => {
                guard.session_workspaces.remove(&victim);
            }
            None => {
                tracing::warn!(
                    session_id = %id,
                    count = guard.session_workspaces.len(),
                    "sandbox: all tracked session workspaces are live; growing past soft cap \
                     rather than relaxing an active session's confinement"
                );
            }
        }
    }
    guard.session_workspaces.insert(id, root);
}

pub fn register_workspace_root_for_session(session_id: &str, path: &Path) {
    if path.as_os_str().is_empty() || session_id.is_empty() {
        return;
    }
    let normalized = resolve_real_path(path);
    let mut guard = confinement().write();
    insert_session_workspace(&mut guard, session_id.to_string(), normalized.clone());
    if guard.workspace.is_none() {
        guard.workspace = Some(normalized);
    }
}

pub fn unregister_session_workspace_root(session_id: &str) {
    let mut guard = confinement().write();
    guard.session_workspaces.remove(session_id);
}

fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path
}

fn normalize_abs(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = PathBuf::new();
    for comp in abs.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    strip_verbatim_prefix(out)
}

fn resolve_real_path(path: &Path) -> PathBuf {
    let abs = normalize_abs(path);
    if let Ok(real) = std::fs::canonicalize(&abs) {
        return strip_verbatim_prefix(real);
    }
    let mut existing = abs.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if std::fs::symlink_metadata(&existing).is_ok() {
            break;
        }
        match existing.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                if !existing.pop() {
                    return abs;
                }
            }
            None => return abs,
        }
    }
    let resolved = std::fs::canonicalize(&existing)
        .map(strip_verbatim_prefix)
        .or_else(|_| {
            std::fs::read_link(&existing).map(|target| {
                let base = existing
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                let joined = if target.is_absolute() {
                    target
                } else {
                    base.join(target)
                };
                normalize_abs(&joined)
            })
        })
        .unwrap_or(existing);
    let mut out = resolved;
    for comp in tail.iter().rev() {
        out.push(comp);
    }
    out
}

fn path_within(target: &Path, root: &Path) -> bool {
    let t = normalize_abs(target);
    let r = normalize_abs(root);
    #[cfg(windows)]
    {
        let ts = t.to_string_lossy().to_lowercase();
        let rs = r.to_string_lossy().to_lowercase();
        let rs_slash_a = format!("{rs}\\");
        let rs_slash_b = format!("{rs}/");
        ts == rs || ts.starts_with(&rs_slash_a) || ts.starts_with(&rs_slash_b)
    }
    #[cfg(not(windows))]
    {
        t == r || t.starts_with(&r)
    }
}

fn is_sensitive_write_target(path: &Path) -> bool {
    let p = normalize_abs(path);
    let s = p.to_string_lossy();

    #[cfg(target_family = "unix")]
    {
        const DENIED: &[&str] = &[
            "/etc/shadow",
            "/etc/passwd",
            "/etc/sudoers",
            "/etc/ssh",
            "/boot",
        ];
        for d in DENIED {
            if s == *d || s.starts_with(&format!("{d}/")) {
                return true;
            }
        }
        if s.starts_with("/proc/") || s.starts_with("/sys/") {
            return true;
        }
        if let Some(home) = std::env::var_os("HOME") {
            let ssh = PathBuf::from(home).join(".ssh");
            if path_within(&p, &ssh) {
                return true;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let low = s.to_lowercase();
        const DENIED: &[&str] = &["c:\\windows\\system32", "c:\\windows\\syswow64"];
        for d in DENIED {
            if low.starts_with(d) {
                return true;
            }
        }
    }

    let _ = s;
    false
}

#[allow(dead_code)]
fn warn_degraded(reason: &str) {
    DEGRADED_WARNED.get_or_init(|| {
        tracing::warn!(
            reason,
            "Sandbox feature is enabled but OS-level isolation is unavailable; \
             falling back to permissive application-layer mode"
        );
    });
}

pub fn is_sandbox_active() -> bool {
    if !cfg!(feature = "sandbox") {
        return false;
    }

    #[cfg(target_os = "linux")]
    {
        #[cfg(feature = "sandbox-landlock")]
        {
            return true;
        }
        #[cfg(not(feature = "sandbox-landlock"))]
        {
            return false;
        }
    }

    #[cfg(target_os = "windows")]
    {
        return cfg!(feature = "sandbox-windows-job");
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

pub fn sandbox_allows_path(path: &Path) -> bool {
    let resolved = resolve_real_path(path);
    let path = resolved.as_path();
    if is_sensitive_write_target(path) {
        tracing::warn!(
            path = %path.display(),
            "sandbox: denied write to sensitive system path (deny-by-default)"
        );
        return false;
    }

    if let Some(svc) = crate::services::try_get_services() {
        let display = path.to_string_lossy().replace('\\', "/");
        if !svc.check_write_path_policy(&display) {
            tracing::warn!(
                path = %path.display(),
                "sandbox: write denied by governance RestrictPaths policy"
            );
            return false;
        }
    }

    {
        let guard = confinement().read();
        if guard.enabled {
            let session_ctx = crate::session::current_session_context();
            let session_root = session_ctx
                .as_ref()
                .and_then(|c| guard.session_workspaces.get(&c.session_id).cloned());
            let session_scoped = session_root.is_some();
            let has_session_ctx = session_ctx.is_some();
            let effective_root = session_root.or_else(|| guard.workspace.clone());
            if let Some(ws) = effective_root.as_ref() {
                if path_within(path, ws) {
                    return true;
                }
                if !has_session_ctx {
                    if let Ok(cwd) = std::env::current_dir() {
                        if path_within(path, &resolve_real_path(&cwd)) {
                            return true;
                        }
                    }
                    if path_within(path, &resolve_real_path(&std::env::temp_dir())) {
                        return true;
                    }
                }
                if guard.allowed_roots.iter().any(|root| path_within(path, root)) {
                    return true;
                }
                tracing::warn!(
                    path = %path.display(),
                    workspace = %ws.display(),
                    session_scoped,
                    "sandbox: write denied outside workspace confinement (deny-by-default); \
                     add the path to [autonomy].allowed_roots or set \
                     [security.sandbox].confine_filesystem=false to permit it"
                );
                return false;
            }
        }
    }

    if !is_sandbox_active() {
        return true;
    }

    #[cfg(target_os = "linux")]
    {

        #[cfg(feature = "sandbox-landlock")]
        {
            let _ = path;
            return true;
        }

        #[cfg(not(feature = "sandbox-landlock"))]
        {
            warn_degraded("Linux sandbox active but sandbox-landlock feature not enabled");
            return path_is_plausible(path);
        }
    }

    #[cfg(target_os = "windows")]
    {

        #[cfg(not(feature = "sandbox-windows-job"))]
        {
            warn_degraded(
                "Windows sandbox active but `sandbox-windows-job` feature not enabled",
            );
        }
        path_is_plausible(path)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        warn_degraded("No OS-level sandbox available on this platform");
        return path_is_plausible(path);
    }
}

fn path_is_plausible(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    #[cfg(target_family = "unix")]
    {
        let denied_prefixes = ["/etc/shadow", "/etc/passwd", "/proc/", "/sys/"];
        for prefix in &denied_prefixes {
            if path_str.starts_with(prefix) {
                tracing::debug!(path = %path_str, "sandbox path-whitelist: denied");
                return false;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let path_lower = path_str.to_lowercase();
        let denied_prefixes = ["c:\\windows\\system32", "c:\\windows\\syswow64"];
        for prefix in &denied_prefixes {
            if path_lower.starts_with(prefix) {
                tracing::debug!(path = %path_str, "sandbox path-whitelist: denied");
                return false;
            }
        }
    }

    true
}
