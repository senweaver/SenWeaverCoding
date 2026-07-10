// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub struct TextBrowserTool {
    security: Arc<SecurityPolicy>,
    preferred_browser: Option<String>,
    timeout_secs: u64,
    max_response_size: usize,
}

const SUPPORTED_BROWSERS: &[&str] = &["lynx", "links", "w3m"];

impl TextBrowserTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        preferred_browser: Option<String>,
        timeout_secs: u64,
    ) -> Self {
        Self {
            security,
            preferred_browser,
            timeout_secs,
            max_response_size: 500_000,
        }
    }

    async fn validate_url(url: &str) -> anyhow::Result<String> {
        let url = url.trim();

        if url.is_empty() {
            anyhow::bail!("URL cannot be empty");
        }

        if url.chars().any(char::is_whitespace) {
            anyhow::bail!("URL cannot contain whitespace");
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            anyhow::bail!("Only http:// and https:// URLs are allowed");
        }

        let host = crate::tools::web::fetch::extract_host(url)?;
        if crate::tools::web::fetch::is_private_or_local_host(&host) {
            anyhow::bail!(
                "Blocked local/private host '{host}': text_browser only renders public web \
                 pages. Use the `browser` or `web_fetch` tool (with allowed_private_hosts) for \
                 local targets."
            );
        }
        let host_for_dns = host.clone();
        tokio::task::spawn_blocking(move || {
            crate::tools::web::fetch::validate_resolved_host_is_public(&host_for_dns)
        })
        .await
        .map_err(|e| anyhow::anyhow!("host validation task failed: {e}"))??;

        Ok(url.to_string())
    }

    fn truncate_response(&self, text: &str) -> String {
        if text.len() > self.max_response_size {
            let mut truncated = text
                .chars()
                .take(self.max_response_size)
                .collect::<String>();
            truncated.push_str("\n\n... [Response truncated due to size limit] ...");
            truncated
        } else {
            text.to_string()
        }
    }

    async fn probe_installed(binary: &str) -> bool {
        let binary = binary.to_string();
        match tokio::task::spawn_blocking(move || which::which(&binary).is_ok()).await {
            Ok(found) => found,
            Err(e) => {
                tracing::warn!(error = %e, "text browser probe task failed");
                false
            }
        }
    }

    async fn detect_browser() -> Option<String> {
        for browser in SUPPORTED_BROWSERS {
            if Self::probe_installed(browser).await {
                return Some((*browser).to_string());
            }
        }
        None
    }

    async fn resolve_browser(&self, requested: Option<&str>) -> anyhow::Result<String> {

        if let Some(browser) = requested {
            let browser = browser.trim().to_lowercase();
            if !SUPPORTED_BROWSERS.contains(&browser.as_str()) {
                anyhow::bail!(
                    "Unsupported text browser '{browser}'. Supported: {}",
                    SUPPORTED_BROWSERS.join(", ")
                );
            }

            if !Self::probe_installed(&browser).await {
                anyhow::bail!("Requested text browser '{browser}' is not installed");
            }
            return Ok(browser);
        }

        if let Some(ref preferred) = self.preferred_browser {
            let preferred = preferred.trim().to_lowercase();
            if SUPPORTED_BROWSERS.contains(&preferred.as_str()) {
                if Self::probe_installed(&preferred).await {
                    return Ok(preferred);
                }
                tracing::warn!(
                    "Configured preferred text browser '{preferred}' is not installed, falling back to auto-detect"
                );
            }
        }

        Self::detect_browser().await.ok_or_else(|| {
            anyhow::anyhow!(
                "No text browser found. Install one of: {}",
                SUPPORTED_BROWSERS.join(", ")
            )
        })
    }

    fn build_dump_args(_browser: &str, url: &str) -> Vec<String> {

        vec!["-dump".to_string(), url.to_string()]
    }
}

#[async_trait]
impl Tool for TextBrowserTool {
    fn name(&self) -> &str {
        "text_browser"
    }

    fn description(&self) -> &str {
        "Render a web page as plain text using a text-based browser (lynx, links, or w3m). \
         Ideal for headless/SSH environments without a graphical browser. \
         Auto-detects available browser or uses a configured preference."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The HTTP or HTTPS URL to render as plain text"
                },
                "browser": {
                    "type": "string",
                    "description": "Text browser to use: \"lynx\", \"links\", or \"w3m\". If omitted, auto-detects an available browser.",
                    "enum": ["lynx", "links", "w3m"]
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

        let url = match Self::validate_url(url).await {
            Ok(v) => v,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                });
            }
        };

        let requested_browser = args.get("browser").and_then(|v| v.as_str());

        let browser = match self.resolve_browser(requested_browser).await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                });
            }
        };

        let dump_args = Self::build_dump_args(&browser, &url);

        let timeout = Duration::from_secs(if self.timeout_secs == 0 {
            tracing::warn!("text_browser: timeout_secs is 0, using safe default of 30s");
            30
        } else {
            self.timeout_secs
        });

        let result = tokio::time::timeout(
            timeout,
            crate::util::hidden_async_command(&browser)
                .args(&dump_args)
                .kill_on_drop(true)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let text = String::from_utf8_lossy(&output.stdout).into_owned();
                    let text = self.truncate_response(&text);
                    Ok(ToolResult {
                        success: true,
                        output: text,
                        error: None,
                    })
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "{browser} exited with status {}: {}",
                            output.status,
                            stderr.trim()
                        )),
                    })
                }
            }
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute {browser}: {e}")),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "{browser} timed out after {} seconds",
                    timeout.as_secs()
                )),
            }),
        }
    }
}
