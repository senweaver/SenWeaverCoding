// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;
use std::sync::OnceLock;

static DEGRADED_WARNED: OnceLock<()> = OnceLock::new();

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
