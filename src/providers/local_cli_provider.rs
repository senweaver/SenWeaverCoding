// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::traits::{ChatRequest, ChatResponse, Provider, TokenUsage};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::time::{Duration, timeout};

pub const LOCAL_CLI_PATH_ENV: &str = "LOCAL_CLI_PATH";

pub const LOCAL_CLI_LEGACY_PATH_ENV: &str = "KILO_CLI_PATH";

const LOCAL_CLI_DEFAULT_CANDIDATES: &[&str] = &["sen-local-cli", "local-cli", "kilo"];

const DEFAULT_MODEL_MARKER: &str = "default";

const LOCAL_CLI_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

const MAX_LOCAL_CLI_STDERR_CHARS: usize = 512;

const LOCAL_CLI_SUPPORTED_TEMPERATURES: [f64; 2] = [0.7, 1.0];
const TEMP_EPSILON: f64 = 1e-9;

pub struct LocalCliProvider {

    binary_path: PathBuf,
}

impl LocalCliProvider {

    pub fn new() -> Self {
        let binary_path = std::env::var(LOCAL_CLI_PATH_ENV)
            .ok()
            .or_else(|| std::env::var(LOCAL_CLI_LEGACY_PATH_ENV).ok())
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                for candidate in LOCAL_CLI_DEFAULT_CANDIDATES {
                    if which::which(candidate).is_ok() {
                        return PathBuf::from(*candidate);
                    }
                }
                PathBuf::from(LOCAL_CLI_DEFAULT_CANDIDATES[0])
            });

        Self { binary_path }
    }

    fn should_forward_model(model: &str) -> bool {
        let trimmed = model.trim();
        !trimmed.is_empty() && trimmed != DEFAULT_MODEL_MARKER
    }

    fn supports_temperature(temperature: f64) -> bool {
        LOCAL_CLI_SUPPORTED_TEMPERATURES
            .iter()
            .any(|v| (temperature - v).abs() < TEMP_EPSILON)
    }

    fn validate_temperature(temperature: f64) -> anyhow::Result<()> {
        if !temperature.is_finite() {
            anyhow::bail!("Local CLI provider received non-finite temperature value");
        }
        if !Self::supports_temperature(temperature) {
            anyhow::bail!(
                "temperature unsupported by local CLI provider: {temperature}. \
                 Supported values: 0.7 or 1.0"
            );
        }
        Ok(())
    }

    fn redact_stderr(stderr: &[u8]) -> String {
        let text = String::from_utf8_lossy(stderr);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if trimmed.chars().count() <= MAX_LOCAL_CLI_STDERR_CHARS {
            return trimmed.to_string();
        }
        let clipped: String = trimmed.chars().take(MAX_LOCAL_CLI_STDERR_CHARS).collect();
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
                "Failed to spawn local CLI provider binary at {}: {err}. \
                 Ensure the CLI is installed and in PATH, or set LOCAL_CLI_PATH.",
                self.binary_path.display()
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(message.as_bytes())
                .await
                .map_err(|err| anyhow::anyhow!("Failed to write prompt to local CLI stdin: {err}"))?;
            stdin
                .shutdown()
                .await
                .map_err(|err| anyhow::anyhow!("Failed to finalize local CLI stdin stream: {err}"))?;
        }

        let output = timeout(LOCAL_CLI_REQUEST_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Local CLI provider request timed out after {:?} (binary: {})",
                    LOCAL_CLI_REQUEST_TIMEOUT,
                    self.binary_path.display()
                )
            })?
            .map_err(|err| anyhow::anyhow!("Local CLI provider process failed: {err}"))?;

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            let stderr_excerpt = Self::redact_stderr(&output.stderr);
            let stderr_note = if stderr_excerpt.is_empty() {
                String::new()
            } else {
                format!(" Stderr: {stderr_excerpt}")
            };
            anyhow::bail!(
                "Local CLI provider exited with non-zero status {code}. \
                 Check that the CLI is authenticated and the CLI is supported.{stderr_note}"
            );
        }

        let text = String::from_utf8(output.stdout)
            .map_err(|err| anyhow::anyhow!("Local CLI provider produced non-UTF-8 output: {err}"))?;

        Ok(text.trim().to_string())
    }
}

impl Default for LocalCliProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LocalCliProvider {
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
