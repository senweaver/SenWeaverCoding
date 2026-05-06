// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Claude Code headless CLI provider.
//!
//! Integrates with the Claude Code CLI, spawning the `claude` binary
//! as a subprocess for each inference request. This allows using Claude's AI
//! models without an interactive UI session.
//!
//! # Usage
//!
//! The `claude` binary must be available in `PATH`, or its location must be
//! set via the `CLAUDE_CODE_PATH` environment variable.
//!
//! Claude Code is invoked as:
//! ```text
//! claude --print -
//! ```
//! with prompt content written to stdin.
//!
//! # Limitations
//!
//! - **System prompt**: The system prompt is prepended to the user message with a
//!   blank-line separator, as the CLI does not provide a dedicated system-prompt flag.
//! - **Temperature**: The CLI does not expose a temperature parameter.
//!   Only default values are accepted; custom values return an explicit error.
//!
//! # Authentication
//!
//! Authentication is handled by Claude Code itself (its own credential store).
//! No explicit API key is required by this provider.
//!
//! # Environment variables
//!
//! - `CLAUDE_CODE_PATH` — override the path to the `claude` binary (default: `"claude"`)

use crate::providers::traits::{ChatMessage, ChatRequest, ChatResponse, Provider, TokenUsage};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

pub const CLAUDE_CODE_PATH_ENV: &str = "CLAUDE_CODE_PATH";

const DEFAULT_CLAUDE_CODE_BINARY: &str = "claude";

const DEFAULT_MODEL_MARKER: &str = "default";

const CLAUDE_CODE_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

const MAX_CLAUDE_CODE_STDERR_CHARS: usize = 512;

const CLAUDE_CODE_SUPPORTED_TEMPERATURES: [f64; 2] = [0.7, 1.0];
const TEMP_EPSILON: f64 = 1e-9;

pub struct ClaudeCodeProvider {

    binary_path: PathBuf,
}

impl ClaudeCodeProvider {

    pub fn new() -> Self {
        let binary_path = std::env::var(CLAUDE_CODE_PATH_ENV)
            .ok()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CLAUDE_CODE_BINARY));

        Self { binary_path }
    }

    fn should_forward_model(model: &str) -> bool {
        let trimmed = model.trim();
        !trimmed.is_empty() && trimmed != DEFAULT_MODEL_MARKER
    }

    fn supports_temperature(temperature: f64) -> bool {
        CLAUDE_CODE_SUPPORTED_TEMPERATURES
            .iter()
            .any(|v| (temperature - v).abs() < TEMP_EPSILON)
    }

    fn validate_temperature(temperature: f64) -> anyhow::Result<f64> {
        if !temperature.is_finite() {
            anyhow::bail!("Claude Code provider received non-finite temperature value");
        }
        if Self::supports_temperature(temperature) {
            return Ok(temperature);
        }

        let clamped = *CLAUDE_CODE_SUPPORTED_TEMPERATURES
            .iter()
            .min_by(|a, b| {
                (temperature - **a)
                    .abs()
                    .partial_cmp(&(temperature - **b).abs())
                    .unwrap()
            })
            .unwrap();
        tracing::debug!(
            requested = temperature,
            clamped = clamped,
            "Clamped unsupported temperature to nearest Claude Code CLI value"
        );
        Ok(clamped)
    }

    fn redact_stderr(stderr: &[u8]) -> String {
        let text = String::from_utf8_lossy(stderr);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if trimmed.chars().count() <= MAX_CLAUDE_CODE_STDERR_CHARS {
            return trimmed.to_string();
        }
        let clipped: String = trimmed.chars().take(MAX_CLAUDE_CODE_STDERR_CHARS).collect();
        format!("{clipped}...")
    }

    async fn invoke_cli(&self, message: &str, model: &str) -> anyhow::Result<String> {
        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("--print");

        if Self::should_forward_model(model) {
            cmd.arg("--model").arg(model);
        }

        cmd.arg("-");
        cmd.kill_on_drop(true);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|err| {
            anyhow::anyhow!(
                "Failed to spawn Claude Code binary at {}: {err}. \
                 Ensure `claude` is installed and in PATH, or set CLAUDE_CODE_PATH.",
                self.binary_path.display()
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(message.as_bytes()).await.map_err(|err| {
                anyhow::anyhow!("Failed to write prompt to Claude Code stdin: {err}")
            })?;
            stdin.shutdown().await.map_err(|err| {
                anyhow::anyhow!("Failed to finalize Claude Code stdin stream: {err}")
            })?;
        }

        let output = timeout(CLAUDE_CODE_REQUEST_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Claude Code request timed out after {:?} (binary: {})",
                    CLAUDE_CODE_REQUEST_TIMEOUT,
                    self.binary_path.display()
                )
            })?
            .map_err(|err| anyhow::anyhow!("Claude Code process failed: {err}"))?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            let stderr_excerpt = Self::redact_stderr(&output.stderr);
            let stderr_note = if stderr_excerpt.is_empty() {
                String::new()
            } else {
                format!(" Stderr: {stderr_excerpt}")
            };
            anyhow::bail!(
                "Claude Code exited with non-zero status {code}. \
                 Check that Claude Code is authenticated and the CLI is supported.{stderr_note}"
            );
        }

        let text = String::from_utf8(output.stdout)
            .map_err(|err| anyhow::anyhow!("Claude Code produced non-UTF-8 output: {err}"))?;

        Ok(text.trim().to_string())
    }
}

impl Default for ClaudeCodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ClaudeCodeProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        Self::validate_temperature(temperature)?;

        let full_message = match system_prompt {
            Some(system) if !system.is_empty() => {
                format!("{system}\n\n{message}")
            }
            _ => message.to_string(),
        };

        self.invoke_cli(&full_message, model).await
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        Self::validate_temperature(temperature)?;

        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.as_str());

        let turns: Vec<&ChatMessage> = messages.iter().filter(|m| m.role != "system").collect();

        if turns.len() <= 1 {
            let last_user = turns.first().map(|m| m.content.as_str()).unwrap_or("");
            let full_message = match system {
                Some(s) if !s.is_empty() => format!("{s}\n\n{last_user}"),
                _ => last_user.to_string(),
            };
            return self.invoke_cli(&full_message, model).await;
        }

        let mut parts = Vec::new();
        if let Some(s) = system {
            if !s.is_empty() {
                parts.push(format!("[system]\n{s}"));
            }
        }
        for msg in &turns {
            let label = match msg.role.as_str() {
                "user" => "[user]",
                "assistant" => "[assistant]",
                other => other,
            };
            parts.push(format!("{label}\n{}", msg.content));
        }
        parts.push("[assistant]".to_string());

        let full_message = parts.join("\n\n");
        self.invoke_cli(&full_message, model).await
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let text = self
            .chat_with_history(request.messages, model, temperature)
            .await?;

        Ok(ChatResponse {
            text: Some(text),
            tool_calls: Vec::new(),
            usage: Some(TokenUsage::default()),
            reasoning_content: None,
        })
    }
}
