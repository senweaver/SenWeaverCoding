// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::RwLock;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(30);

static CACHE: RwLock<Option<(Instant, Option<DetectedSystemProxy>)>> = RwLock::new(None);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedSystemProxy {
    pub http: Option<String>,
    pub https: Option<String>,
    pub all: Option<String>,
    pub bypass: Vec<String>,
}

impl DetectedSystemProxy {
    pub fn is_empty(&self) -> bool {
        self.http.is_none() && self.https.is_none() && self.all.is_none()
    }

    pub fn signature(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.http.as_deref().unwrap_or("-"),
            self.https.as_deref().unwrap_or("-"),
            self.all.as_deref().unwrap_or("-"),
            self.bypass.join(",")
        )
    }
}

pub fn detect_cached() -> Option<DetectedSystemProxy> {
    if let Ok(guard) = CACHE.read() {
        if let Some((at, value)) = guard.as_ref() {
            if at.elapsed() < CACHE_TTL {
                return value.clone();
            }
        }
    }
    let fresh = detect_uncached();
    if let Ok(mut guard) = CACHE.write() {
        *guard = Some((Instant::now(), fresh.clone()));
    }
    fresh
}

pub fn invalidate() {
    if let Ok(mut guard) = CACHE.write() {
        *guard = None;
    }
}

fn detect_uncached() -> Option<DetectedSystemProxy> {
    #[cfg(windows)]
    {
        return detect_windows();
    }
    #[cfg(target_os = "macos")]
    {
        return detect_macos();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return detect_linux();
    }
    #[allow(unreachable_code)]
    {
        None
    }
}

#[cfg(windows)]
fn with_scheme(addr: &str, default_scheme: &str) -> String {
    let trimmed = addr.trim();
    if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("{default_scheme}://{trimmed}")
    }
}

#[cfg(windows)]
fn detect_windows() -> Option<DetectedSystemProxy> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;

    let enable: u32 = settings.get_value("ProxyEnable").unwrap_or(0);
    if enable == 0 {
        return None;
    }

    let server: String = settings.get_value("ProxyServer").ok()?;
    let server = server.trim().to_string();
    if server.is_empty() {
        return None;
    }

    let mut proxy = DetectedSystemProxy::default();
    if server.contains('=') {
        for part in server.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((proto, addr)) = part.split_once('=') {
                let addr = addr.trim();
                if addr.is_empty() {
                    continue;
                }
                match proto.trim().to_ascii_lowercase().as_str() {
                    "http" => proxy.http = Some(with_scheme(addr, "http")),
                    "https" => proxy.https = Some(with_scheme(addr, "http")),
                    "socks" => proxy.all = Some(with_scheme(addr, "socks5")),
                    _ => {}
                }
            }
        }
    } else {
        proxy.all = Some(with_scheme(&server, "http"));
    }

    if let Ok(override_value) = settings.get_value::<String, _>("ProxyOverride") {
        proxy.bypass = parse_bypass(&override_value);
    }

    if proxy.is_empty() {
        None
    } else {
        Some(proxy)
    }
}

#[cfg(windows)]
fn parse_bypass(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for entry in raw.split([';', ',']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if entry.eq_ignore_ascii_case("<local>") {
            out.push("localhost".to_string());
            out.push("127.0.0.1".to_string());
            out.push("::1".to_string());
            continue;
        }
        out.push(entry.to_string());
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(target_os = "macos")]
fn detect_macos() -> Option<DetectedSystemProxy> {
    let output = crate::util::hidden_sync_command("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let enabled = |key: &str| map.get(key).map(|v| v == "1").unwrap_or(false);
    let value = |key: &str| map.get(key).cloned();

    let mut proxy = DetectedSystemProxy::default();
    if enabled("HTTPEnable") {
        if let (Some(host), Some(port)) = (value("HTTPProxy"), value("HTTPPort")) {
            proxy.http = Some(format!("http://{host}:{port}"));
        }
    }
    if enabled("HTTPSEnable") {
        if let (Some(host), Some(port)) = (value("HTTPSProxy"), value("HTTPSPort")) {
            proxy.https = Some(format!("http://{host}:{port}"));
        }
    }
    if enabled("SOCKSEnable") {
        if let (Some(host), Some(port)) = (value("SOCKSProxy"), value("SOCKSPort")) {
            proxy.all = Some(format!("socks5://{host}:{port}"));
        }
    }

    if proxy.is_empty() {
        None
    } else {
        Some(proxy)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_linux() -> Option<DetectedSystemProxy> {
    let mode = run_gsettings(&["get", "org.gnome.system.proxy", "mode"])?;
    if !mode.contains("manual") {
        return None;
    }

    let mut proxy = DetectedSystemProxy::default();
    if let Some(url) = gsettings_endpoint("org.gnome.system.proxy.http", "http") {
        proxy.http = Some(url);
    }
    if let Some(url) = gsettings_endpoint("org.gnome.system.proxy.https", "http") {
        proxy.https = Some(url);
    }
    if let Some(url) = gsettings_endpoint("org.gnome.system.proxy.socks", "socks5") {
        proxy.all = Some(url);
    }

    if proxy.is_empty() {
        None
    } else {
        Some(proxy)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn gsettings_endpoint(schema: &str, scheme: &str) -> Option<String> {
    let host = run_gsettings(&["get", schema, "host"]).map(|v| strip_gsettings_string(&v))?;
    if host.is_empty() {
        return None;
    }
    let port = run_gsettings(&["get", schema, "port"])?;
    let port = port.trim();
    if port.is_empty() || port == "0" {
        return None;
    }
    Some(format!("{scheme}://{host}:{port}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_gsettings(args: &[&str]) -> Option<String> {
    let output = crate::util::hidden_sync_command("gsettings")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn strip_gsettings_string(raw: &str) -> String {
    raw.trim().trim_matches('\'').trim_matches('"').to_string()
}
