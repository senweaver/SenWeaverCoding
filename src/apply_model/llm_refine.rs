// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::prompts::{REFINE_SYSTEM_PROMPT, build_refine_user_prompt, build_refine_user_prompt_v2};
use super::traits::ApplyError;
use crate::providers::traits::Provider;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureKind {

    ContextMismatch,

    LineDrift { delta: i32 },

    TreeSitterError { node_kind: String, line: u32 },

    CompileError { code: Option<String>, line: u32 },

    BracketUnbalanced { line: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousAttempt {

    pub diff: String,

    pub error: String,
}

#[async_trait]
pub trait LlmRefiner: Send + Sync {

    async fn refine(
        &self,
        source: &str,
        failed_diff: &str,
        hint: Option<&str>,
    ) -> Result<String, ApplyError>;

    async fn refine_with_context(
        &self,
        source: &str,
        failed_diff: &str,
        hint: Option<&str>,
        _failure: Option<&FailureKind>,
        _prev: Option<&PreviousAttempt>,
        _attempt_idx: u8,
    ) -> Result<String, ApplyError> {
        self.refine(source, failed_diff, hint).await
    }

    fn max_recursive_attempts(&self) -> u8 {
        2
    }

    /// Full-file merge (Morph/Relace-style fast-apply): given the original file and
    /// a lazy edit snippet (which may use `// ... existing code ...` markers), return
    /// the COMPLETE merged file. This is the last-resort fallback when diff-based
    /// application keeps failing — the model reasons over whole content instead of
    /// re-emitting a fragile patch. Default: unsupported.
    async fn merge_full_file(
        &self,
        _source: &str,
        _edit_snippet: &str,
        _instruction: Option<&str>,
    ) -> Result<String, ApplyError> {
        Err(ApplyError::LlmError(
            "full-file merge not supported by this refiner".to_string(),
        ))
    }

    fn supports_full_file_merge(&self) -> bool {
        false
    }

    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct ScriptedRefiner {
    pub replacement_diff: String,
}

#[async_trait]
impl LlmRefiner for ScriptedRefiner {
    async fn refine(
        &self,
        _source: &str,
        _failed_diff: &str,
        _hint: Option<&str>,
    ) -> Result<String, ApplyError> {
        Ok(self.replacement_diff.clone())
    }
    fn name(&self) -> &'static str {
        "scripted_refiner"
    }
}

pub struct HttpLlmRefiner {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,

    pub timeout: Duration,

    pub max_recursive_attempts: u8,

    pub temperature_step: f64,
}

impl HttpLlmRefiner {
    #[must_use]
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature: 0.0,
            timeout: Duration::from_secs(30),
            max_recursive_attempts: 2,
            temperature_step: 0.1,
        }
    }

    #[must_use]
    pub fn with_temperature(mut self, temp: f64) -> Self {
        self.temperature = temp;
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_max_recursive_attempts(mut self, n: u8) -> Self {
        self.max_recursive_attempts = n;
        self
    }

    #[must_use]
    pub fn with_temperature_step(mut self, step: f64) -> Self {
        self.temperature_step = step;
        self
    }

    fn temperature_for(&self, attempt_idx: u8) -> f64 {
        let bumped = self.temperature + (attempt_idx as f64) * self.temperature_step;
        let max_temp = self.temperature + 0.2;
        if bumped > max_temp {
            max_temp
        } else {
            bumped
        }
    }

    async fn execute_request(&self, user: String, temperature: f64) -> Result<String, ApplyError> {
        crate::observability::code_intel_metrics::incr_apply_model_refine_attempt();
        let fut = self
            .provider
            .chat_with_system(Some(REFINE_SYSTEM_PROMPT), &user, &self.model, temperature);
        let reply = match tokio::time::timeout(self.timeout, fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(err)) => {
                crate::observability::code_intel_metrics::incr_apply_model_refine_failed();
                return Err(ApplyError::LlmError(format!(
                    "refine provider error: {err}"
                )));
            }
            Err(_) => {
                crate::observability::code_intel_metrics::incr_apply_model_refine_failed();
                return Err(ApplyError::LlmError(format!(
                    "refine timed out after {:?}",
                    self.timeout
                )));
            }
        };
        let stripped = strip_markdown_fence(&reply);
        if stripped.trim().is_empty() {
            crate::observability::code_intel_metrics::incr_apply_model_refine_failed();
            return Err(ApplyError::LlmError(
                "refine returned empty diff".to_string(),
            ));
        }
        crate::observability::code_intel_metrics::incr_apply_model_refine_success();
        Ok(stripped)
    }
}

impl std::fmt::Debug for HttpLlmRefiner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpLlmRefiner")
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("timeout", &self.timeout)
            .field("max_recursive_attempts", &self.max_recursive_attempts)
            .field("temperature_step", &self.temperature_step)
            .finish()
    }
}

#[async_trait]
impl LlmRefiner for HttpLlmRefiner {
    async fn refine(
        &self,
        source: &str,
        failed_diff: &str,
        hint: Option<&str>,
    ) -> Result<String, ApplyError> {
        let user = build_refine_user_prompt(source, failed_diff, hint);
        self.execute_request(user, self.temperature).await
    }

    async fn refine_with_context(
        &self,
        source: &str,
        failed_diff: &str,
        hint: Option<&str>,
        failure: Option<&FailureKind>,
        prev: Option<&PreviousAttempt>,
        attempt_idx: u8,
    ) -> Result<String, ApplyError> {

        crate::observability::code_intel_metrics::incr_apply_model_refine_recursive_attempt();
        let user = build_refine_user_prompt_v2(source, failed_diff, hint, failure, prev);
        let temp = self.temperature_for(attempt_idx);
        self.execute_request(user, temp).await
    }

    fn max_recursive_attempts(&self) -> u8 {
        self.max_recursive_attempts
    }

    async fn merge_full_file(
        &self,
        source: &str,
        edit_snippet: &str,
        instruction: Option<&str>,
    ) -> Result<String, ApplyError> {
        let user = build_full_file_merge_prompt(source, edit_snippet, instruction);
        // The merge system prompt asks for the whole file verbatim, no fences.
        let fut = self.provider.chat_with_system(
            Some(FULL_FILE_MERGE_SYSTEM_PROMPT),
            &user,
            &self.model,
            self.temperature,
        );
        let reply = match tokio::time::timeout(self.timeout, fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(err)) => {
                return Err(ApplyError::LlmError(format!("full-file merge error: {err}")));
            }
            Err(_) => {
                return Err(ApplyError::LlmError(format!(
                    "full-file merge timed out after {:?}",
                    self.timeout
                )));
            }
        };
        let merged = strip_markdown_fence(&reply);
        if merged.trim().is_empty() {
            return Err(ApplyError::LlmError(
                "full-file merge returned empty output".to_string(),
            ));
        }
        Ok(merged)
    }

    fn supports_full_file_merge(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "http_llm_refiner"
    }
}

const FULL_FILE_MERGE_SYSTEM_PROMPT: &str =
    "You merge a lazy code edit into a source file and return the COMPLETE resulting file. \
     Expand any `// ... existing code ...` (or similar) markers back into the original code \
     they stand for. Output ONLY the full merged file content with no explanations, no \
     markdown fences, and no commentary. Preserve the original file's indentation, line \
     endings, and any unedited regions exactly.";

fn build_full_file_merge_prompt(
    source: &str,
    edit_snippet: &str,
    instruction: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(instr) = instruction.filter(|s| !s.trim().is_empty()) {
        out.push_str("<instruction>\n");
        out.push_str(instr.trim());
        out.push_str("\n</instruction>\n");
    }
    out.push_str("<original_file>\n");
    out.push_str(source);
    out.push_str("\n</original_file>\n<edit_snippet>\n");
    out.push_str(edit_snippet);
    out.push_str("\n</edit_snippet>\n\nReturn the complete merged file:");
    out
}

fn strip_markdown_fence(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let without_fence = if let Some(rest) = trimmed.strip_prefix("```diff") {
        rest.trim_start_matches('\n')
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.trim_start_matches('\n')
    } else {
        trimmed
    };

    let end_trimmed = without_fence.trim_end();
    let core = end_trimmed
        .strip_suffix("```")
        .map_or(end_trimmed, str::trim_end);

    if core.ends_with('\n') {
        core.to_string()
    } else {
        format!("{core}\n")
    }
}
