// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct BrowserOpenTool {
    security: Arc<SecurityPolicy>,
    allowed_domains: Vec<String>,
}

impl BrowserOpenTool {
    pub fn new(security: Arc<SecurityPolicy>, allowed_domains: Vec<String>) -> Self {
        Self {
            security,
            allowed_domains: normalize_allowed_domains(allowed_domains),
        }
    }

    fn validate_url(&self, raw_url: &str) -> anyhow::Result<String> {
        let url = raw_url.trim();

        if url.is_empty() {
            anyhow::bail!("URL cannot be empty");
        }

        if url.chars().any(char::is_whitespace) {
            anyhow::bail!("URL cannot contain whitespace");
        }

        if !url.starts_with("https://") {
            anyhow::bail!("Only https:// URLs are allowed");
        }

        if self.allowed_domains.is_empty() {
            anyhow::bail!(
                "Browser tool is enabled but no allowed_domains are configured. Add [browser].allowed_domains in config.toml"
            );
        }

        let host = extract_host(url)?;

        if is_private_or_local_host(&host) {
            anyhow::bail!("Blocked local/private host: {host}");
        }

        if !host_matches_allowlist(&host, &self.allowed_domains) {
            anyhow::bail!("Host '{host}' is not in browser.allowed_domains");
        }

        Ok(url.to_string())
    }
}

#[async_trait]
impl Tool for BrowserOpenTool {
    fn name(&self) -> &str {
        "browser_open"
    }

    fn description(&self) -> &str {
        "Open an approved HTTPS URL in the **external system browser** (Chrome / Edge / Safari / default OS handler). \
         On desktop the user has the built-in `browser` tool (visible embedded dock)  - ALWAYS prefer that; only fall \
         back to this tool when no embedded dock is available (pure CLI / headless server) or when the user \
         explicitly asks for the system browser. Never call this in the same turn as a successful `browser` with \
         action='open' for the same URL. Security constraints: allowlist-only domains, no local/private hosts, no bulk content extraction."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTPS URL to open in the system browser"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: rate limit exceeded".into()),
            });
        }

        let url = match self.validate_url(url) {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                });
            }
        };

        match open_in_system_browser(&url).await {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!("Opened in system browser: {url}"),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to open system browser: {e}")),
            }),
        }
    }
}

async fn open_in_system_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let primary_error = match crate::util::hidden_async_command("open").arg(url).status().await {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => format!("open exited with status {status}"),
            Err(error) => format!("open not runnable: {error}"),
        };

        let mut brave_error = String::new();
        for app in ["Brave Browser", "Brave"] {
            match crate::util::hidden_async_command("open")
                .arg("-a")
                .arg(app)
                .arg(url)
                .status()
                .await
            {
                Ok(status) if status.success() => return Ok(()),
                Ok(status) => {
                    brave_error = format!("open -a '{app}' exited with status {status}");
                }
                Err(error) => {
                    brave_error = format!("open -a '{app}' not runnable: {error}");
                }
            }
        }

        anyhow::bail!(
            "Failed to open URL with default browser launcher: {primary_error}. Brave compatibility fallback also failed: {brave_error}"
        );
    }

    #[cfg(target_os = "linux")]
    {
        let mut last_error = String::new();
        for cmd in [
            "xdg-open",
            "gio",
            "sensible-browser",
            "brave-browser",
            "brave",
        ] {
            let mut command = crate::util::hidden_async_command(cmd);
            if cmd == "gio" {
                command.arg("open");
            }
            command.arg(url);
            match command.status().await {
                Ok(status) if status.success() => return Ok(()),
                Ok(status) => {
                    last_error = format!("{cmd} exited with status {status}");
                }
                Err(error) => {
                    last_error = format!("{cmd} not runnable: {error}");
                }
            }
        }

        anyhow::bail!(
            "Failed to open URL with default browser launchers; Brave compatibility fallback also failed. Last error: {last_error}"
        );
    }

    #[cfg(target_os = "windows")]
    {

        let primary_error = match crate::util::hidden_async_command("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .status()
            .await
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => format!("rundll32 default-browser launcher exited with status {status}"),
            Err(error) => format!("rundll32 default-browser launcher not runnable: {error}"),
        };

        let mut brave_error = String::new();
        for cmd in ["brave", "brave.exe"] {
            match crate::util::hidden_async_command(cmd).arg(url).status().await {
                Ok(status) if status.success() => return Ok(()),
                Ok(status) => {
                    brave_error = format!("{cmd} exited with status {status}");
                }
                Err(error) => {
                    brave_error = format!("{cmd} not runnable: {error}");
                }
            }
        }

        anyhow::bail!(
            "Failed to open URL with default browser launcher: {primary_error}. Brave compatibility fallback also failed: {brave_error}"
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        anyhow::bail!("browser_open is not supported on this OS");
    }
}

fn normalize_allowed_domains(domains: Vec<String>) -> Vec<String> {
    let mut normalized = domains
        .into_iter()
        .filter_map(|d| normalize_domain(&d))
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}

fn normalize_domain(raw: &str) -> Option<String> {
    let mut d = raw.trim().to_lowercase();
    if d.is_empty() {
        return None;
    }

    if let Some(stripped) = d.strip_prefix("https://") {
        d = stripped.to_string();
    } else if let Some(stripped) = d.strip_prefix("http://") {
        d = stripped.to_string();
    }

    if let Some((host, _)) = d.split_once('/') {
        d = host.to_string();
    }

    d = d.trim_start_matches('.').trim_end_matches('.').to_string();

    if let Some((host, _)) = d.split_once(':') {
        d = host.to_string();
    }

    if d.is_empty() || d.chars().any(char::is_whitespace) {
        return None;
    }

    Some(d)
}

fn extract_host(url: &str) -> anyhow::Result<String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| anyhow::anyhow!("Only https:// URLs are allowed"))?;

    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid URL"))?;

    if authority.is_empty() {
        anyhow::bail!("URL must include a host");
    }

    if authority.contains('@') {
        anyhow::bail!("URL userinfo is not allowed");
    }

    if authority.starts_with('[') {
        anyhow::bail!("IPv6 hosts are not supported in browser_open");
    }

    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.')
        .to_lowercase();

    if host.is_empty() {
        anyhow::bail!("URL must include a valid host");
    }

    Ok(host)
}

fn host_matches_allowlist(host: &str, allowed_domains: &[String]) -> bool {
    if allowed_domains.iter().any(|domain| domain == "*") {
        return true;
    }

    allowed_domains.iter().any(|domain| {
        host == domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn is_private_or_local_host(host: &str) -> bool {
    let has_local_tld = host
        .rsplit('.')
        .next()
        .is_some_and(|label| label == "local");

    if host == "localhost" || host.ends_with(".localhost") || has_local_tld || host == "::1" {
        return true;
    }

    if let Some([a, b, _, _]) = parse_ipv4(host) {
        return a == 0
            || a == 10
            || a == 127
            || (a == 169 && b == 254)
            || (a == 172 && (16..=31).contains(&b))
            || (a == 192 && b == 168)
            || (a == 100 && (64..=127).contains(&b));
    }

    false
}

fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return None;
    }

    let mut octets = [0_u8; 4];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = part.parse::<u8>().ok()?;
    }
    Some(octets)
}
