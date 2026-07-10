// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{ChatRequest, ChatResponse, Provider, TokenUsage};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::time::{Duration, timeout};

pub const GEMINI_CLI_PATH_ENV: &str = "GEMINI_CLI_PATH";

const DEFAULT_GEMINI_CLI_BINARY: &str = "gemini";

const DEFAULT_MODEL_MARKER: &str = "default";

const GEMINI_CLI_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

const MAX_GEMINI_CLI_STDERR_CHARS: usize = 512;

const GEMINI_CLI_SUPPORTED_TEMPERATURES: [f64; 2] = [0.7, 1.0];
const TEMP_EPSILON: f64 = 1e-9;

pub struct GeminiCliProvider {

    binary_path: PathBuf,
}

impl GeminiCliProvider {

    pub fn new() -> Self {
        let binary_path = std::env::var(GEMINI_CLI_PATH_ENV)
            .ok()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_GEMINI_CLI_BINARY));

        Self { binary_path }
    }

    fn should_forward_model(model: &str) -> bool {
        let trimmed = model.trim();
        !trimmed.is_empty() && trimmed != DEFAULT_MODEL_MARKER
    }

    fn supports_temperature(temperature: f64) -> bool {
        GEMINI_CLI_SUPPORTED_TEMPERATURES
            .iter()
            .any(|v| (temperature - v).abs() < TEMP_EPSILON)
    }

    fn validate_temperature(temperature: f64) -> anyhow::Result<f64> {
        if !temperature.is_finite() {
            anyhow::bail!("Gemini CLI provider received non-finite temperature value");
        }
        if Self::supports_temperature(temperature) {
            return Ok(temperature);
        }
        let clamped = *GEMINI_CLI_SUPPORTED_TEMPERATURES
            .iter()
            .min_by(|a, b| {
                (temperature - **a)
                    .abs()
                    .partial_cmp(&(temperature - **b).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or_else(|| {
                anyhow::anyhow!("Gemini CLI provider has no supported temperatures configured")
            })?;
        tracing::debug!(
            requested = temperature,
            clamped = clamped,
            "Clamped unsupported temperature to nearest Gemini CLI value"
        );
        Ok(clamped)
    }

    fn redact_stderr(stderr: &[u8]) -> String {
        let text = String::from_utf8_lossy(stderr);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if trimmed.chars().count() <= MAX_GEMINI_CLI_STDERR_CHARS {
            return trimmed.to_string();
        }
        let clipped: String = trimmed.chars().take(MAX_GEMINI_CLI_STDERR_CHARS).collect();
        format!("{clipped}...")
    }

    async fn invoke_cli(&self, message: &str, model: &str) -> anyhow::Result<String> {
        let mut cmd = crate::util::hidden_async_command(&self.binary_path);
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
                "Failed to spawn Gemini CLI binary at {}: {err}. \
                 Ensure `gemini` is installed and in PATH, or set GEMINI_CLI_PATH.",
                self.binary_path.display()
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(message.as_bytes()).await.map_err(|err| {
                anyhow::anyhow!("Failed to write prompt to Gemini CLI stdin: {err}")
            })?;
            stdin.shutdown().await.map_err(|err| {
                anyhow::anyhow!("Failed to finalize Gemini CLI stdin stream: {err}")
            })?;
        }

        let output = timeout(GEMINI_CLI_REQUEST_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Gemini CLI request timed out after {:?} (binary: {})",
                    GEMINI_CLI_REQUEST_TIMEOUT,
                    self.binary_path.display()
                )
            })?
            .map_err(|err| anyhow::anyhow!("Gemini CLI process failed: {err}"))?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            let stderr_excerpt = Self::redact_stderr(&output.stderr);
            let stderr_note = if stderr_excerpt.is_empty() {
                String::new()
            } else {
                format!(" Stderr: {stderr_excerpt}")
            };
            anyhow::bail!(
                "Gemini CLI exited with non-zero status {code}. \
                 Check that Gemini CLI is authenticated and the CLI is supported.{stderr_note}"
            );
        }

        let text = String::from_utf8(output.stdout)
            .map_err(|err| anyhow::anyhow!("Gemini CLI produced non-UTF-8 output: {err}"))?;

        Ok(text.trim().to_string())
    }
}

impl Default for GeminiCliProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for GeminiCliProvider {
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

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let text = self
            .chat_with_history(request.messages, model, temperature)
            .await?;

        Ok(ChatResponse::text_only(
            Some(text),
            Some(TokenUsage::default()),
        ))
    }
}
