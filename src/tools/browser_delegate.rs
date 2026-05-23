// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::security::SecurityPolicy;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use tokio::time::{Duration, timeout};

static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https?://[^\s\)\]\},\"'`<>]+"#).expect("browser_delegate URL regex must compile")
});

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BrowserDelegateConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_browser_cli")]
    pub cli_binary: String,

    #[serde(default)]
    pub chrome_profile_dir: String,

    #[serde(default)]
    pub allowed_domains: Vec<String>,

    #[serde(default)]
    pub blocked_domains: Vec<String>,

    #[serde(default = "default_browser_task_timeout")]
    pub task_timeout_secs: u64,
}

fn default_browser_cli() -> String {
    "claude".into()
}

fn default_browser_task_timeout() -> u64 {
    120
}

impl Default for BrowserDelegateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cli_binary: default_browser_cli(),
            chrome_profile_dir: String::new(),
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            task_timeout_secs: default_browser_task_timeout(),
        }
    }
}

pub struct BrowserDelegateTool {
    security: Arc<SecurityPolicy>,
    config: BrowserDelegateConfig,
}

impl BrowserDelegateTool {

    pub fn new(security: Arc<SecurityPolicy>, config: BrowserDelegateConfig) -> Self {
        Self { security, config }
    }

    fn build_command(&self, task: &str, url: Option<&str>) -> tokio::process::Command {
        let mut cmd = crate::util::hidden_async_command(&self.config.cli_binary);

        cmd.arg("--print");

        let prompt = if let Some(url) = url {
            format!(
                "Use your browser tools to navigate to {} and perform the following task: {}",
                url, task
            )
        } else {
            format!(
                "Use your browser tools to perform the following task: {}",
                task
            )
        };

        cmd.arg(&prompt);

        if !self.config.chrome_profile_dir.is_empty() {
            cmd.env("CHROME_USER_DATA_DIR", &self.config.chrome_profile_dir);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd
    }

    fn validate_task_urls(&self, task: &str) -> anyhow::Result<()> {
        for m in URL_RE.find_iter(task) {
            self.validate_url(m.as_str())?;
        }
        Ok(())
    }

    fn validate_url(&self, url: &str) -> anyhow::Result<()> {
        let parsed = url
            .parse::<reqwest::Url>()
            .map_err(|e| anyhow::anyhow!("invalid URL '{}': {}", url, e))?;

        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            anyhow::bail!("unsupported URL scheme: {}", scheme);
        }

        let domain = parsed.host_str().unwrap_or("").to_string();

        if domain.is_empty() {
            anyhow::bail!("URL has no host: {}", url);
        }

        for blocked in &self.config.blocked_domains {
            if domain_matches(&domain, blocked) {
                anyhow::bail!("domain '{}' is blocked by browser_delegate policy", domain);
            }
        }

        if !self.config.allowed_domains.is_empty() {
            let allowed = self
                .config
                .allowed_domains
                .iter()
                .any(|d| domain_matches(&domain, d));
            if !allowed {
                anyhow::bail!(
                    "domain '{}' is not in browser_delegate allowed_domains",
                    domain
                );
            }
        }

        Ok(())
    }
}

fn domain_matches(domain: &str, pattern: &str) -> bool {
    let d = domain.to_lowercase();
    let p = pattern.to_lowercase();
    d == p || d.ends_with(&format!(".{}", p))
}

const MAX_STDERR_CHARS: usize = 512;

const VALID_EXTRACT_FORMATS: &[&str] = &["text", "json", "summary"];

#[async_trait]
impl Tool for BrowserDelegateTool {
    fn name(&self) -> &str {
        "browser_delegate"
    }

    fn description(&self) -> &str {
        "Delegate browser-based tasks to an **external browser-capable CLI subprocess** (e.g. Claude Code with claude-in-chrome) \
         for interacting with corporate web apps (Teams, Outlook, Jira, Confluence) that need a persistent SSO Chrome profile. \
         This spawns an external CLI and external Chrome — on desktop ALWAYS prefer the built-in `browser` tool (embedded dock) \
         for ordinary browsing; only use this when the task truly requires a logged-in external Chrome profile."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Description of the browser task to perform"
                },
                "url": {
                    "type": "string",
                    "description": "Optional URL to navigate to before performing the task"
                },
                "extract_format": {
                    "type": "string",
                    "enum": ["text", "json", "summary"],
                    "description": "Desired output format (default: text)"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("browser_delegate tool is denied by security policy".into()),
            });
        }
        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("browser_delegate action rate-limited".into()),
            });
        }

        let _resource_guard = match crate::session::acquire_browser_for_current_session().await {
            Some(Ok(g)) => Some(g),
            Some(Err(e)) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
            None => None,
        };

        let task = args
            .get("task")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();

        if task.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("'task' parameter is required and cannot be empty".into()),
            });
        }

        let url = args
            .get("url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|u| !u.is_empty());

        if let Some(url) = url {
            if let Err(e) = self.validate_url(url) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("URL validation failed: {e}")),
                });
            }
        }

        if let Err(e) = self.validate_task_urls(task) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("task text contains a disallowed URL: {e}")),
            });
        }

        let extract_format = args
            .get("extract_format")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("text");

        if !VALID_EXTRACT_FORMATS.contains(&extract_format) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "unsupported extract_format '{}': allowed values are 'text', 'json', 'summary'",
                    extract_format
                )),
            });
        }

        let full_task = match extract_format {
            "json" => format!("{task}. Return the result as structured JSON."),
            "summary" => format!("{task}. Return a concise summary."),
            _ => task.to_string(),
        };

        let mut cmd = self.build_command(&full_task, url);

        cmd.kill_on_drop(true);

        let deadline = Duration::from_secs(self.config.task_timeout_secs);
        let result = timeout(deadline, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stderr_truncated: String = stderr.chars().take(MAX_STDERR_CHARS).collect();

                if output.status.success() {
                    Ok(ToolResult {
                        success: true,
                        output: stdout,
                        error: if stderr_truncated.is_empty() {
                            None
                        } else {
                            Some(stderr_truncated)
                        },
                    })
                } else {
                    Ok(ToolResult {
                        success: false,
                        output: stdout,
                        error: Some(format!(
                            "CLI exited with status {}: {}",
                            output.status, stderr_truncated
                        )),
                    })
                }
            }
            Ok(Err(e)) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("failed to spawn browser CLI: {e}")),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "browser task timed out after {}s",
                    self.config.task_timeout_secs
                )),
            }),
        }
    }
}

pub struct BrowserTaskTemplates;

impl BrowserTaskTemplates {

    pub fn read_teams_messages(channel: &str, count: usize) -> String {
        format!(
            "Open Microsoft Teams, navigate to the '{}' channel, \
             read the last {} messages, and return them as a structured \
             summary with sender, timestamp, and message content.",
            channel, count
        )
    }

    pub fn read_outlook_inbox(count: usize) -> String {
        format!(
            "Open Outlook Web (outlook.office.com), go to the inbox, \
             read the last {} emails, and return a summary of each with \
             sender, subject, date, and first 2 lines of body.",
            count
        )
    }

    pub fn read_jira_board(project: &str) -> String {
        format!(
            "Open Jira, navigate to the '{}' project board, and return \
             the current sprint tickets with their status, assignee, and title.",
            project
        )
    }

    pub fn read_confluence_page(url: &str) -> String {
        format!(
            "Open the Confluence page at {}, read the full content, \
             and return a structured summary.",
            url
        )
    }
}
