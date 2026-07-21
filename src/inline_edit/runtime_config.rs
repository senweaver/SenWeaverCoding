// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::verification::{
    LspPoolDiagnosticFetcher, LspDiagVerifier, SyntacticVerifier, TestRunnerBuilder,
    VerificationPipeline, VerificationPolicy, Verifier,
};
use crate::apply_model::llm_refine::HttpLlmRefiner;
use crate::providers::Provider;
use crate::services::lsp::LspService;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub apply_model: ApplyModelSection,
    #[serde(default)]
    pub verification: VerificationSection,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplyModelSection {
    #[serde(default)]
    pub refine: RefineSection,

    #[serde(default)]
    pub code_edit: CodeEditSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefineSection {

    #[serde(default = "default_refine_temperature")]
    pub temperature: f64,

    #[serde(default = "default_refine_timeout_seconds")]
    pub timeout_seconds: u64,

    #[serde(default = "default_max_refine")]
    pub max_refine: u8,

    #[serde(default = "default_max_recursive")]
    pub max_recursive: u8,
}

impl Default for RefineSection {
    fn default() -> Self {
        Self {
            temperature: default_refine_temperature(),
            timeout_seconds: default_refine_timeout_seconds(),
            max_refine: default_max_refine(),
            max_recursive: default_max_recursive(),
        }
    }
}

fn default_refine_temperature() -> f64 {
    0.0
}
fn default_refine_timeout_seconds() -> u64 {
    30
}
fn default_max_refine() -> u8 {
    1
}
fn default_max_recursive() -> u8 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEditSection {

    #[serde(default = "default_auto_expand_deps")]
    pub auto_expand_deps: bool,

    #[serde(default = "default_max_parallel_per_layer")]
    pub max_parallel_per_layer: usize,

    #[serde(default = "default_full_file_rewrite_max_lines")]
    pub full_file_rewrite_max_lines: usize,

    #[serde(default = "default_window_prompt_min_lines")]
    pub window_prompt_min_lines: usize,

    #[serde(default = "default_per_step_timeout_seconds")]
    pub per_step_timeout_seconds: u64,

    #[serde(default = "default_max_fix_attempts")]
    pub max_fix_attempts: u32,
}

impl Default for CodeEditSection {
    fn default() -> Self {
        Self {
            auto_expand_deps: default_auto_expand_deps(),
            max_parallel_per_layer: default_max_parallel_per_layer(),
            full_file_rewrite_max_lines: default_full_file_rewrite_max_lines(),
            window_prompt_min_lines: default_window_prompt_min_lines(),
            per_step_timeout_seconds: default_per_step_timeout_seconds(),
            max_fix_attempts: default_max_fix_attempts(),
        }
    }
}

fn default_auto_expand_deps() -> bool {
    true
}
fn default_max_parallel_per_layer() -> usize {
    4
}
fn default_full_file_rewrite_max_lines() -> usize {
    150
}
fn default_window_prompt_min_lines() -> usize {
    1_000
}
fn default_per_step_timeout_seconds() -> u64 {
    120
}
fn default_max_fix_attempts() -> u32 {
    5
}

pub fn build_code_edit_config(cfg: &ApplyModelSection) -> CodeEditSection {
    cfg.code_edit.clone()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationSection {

    #[serde(default = "default_stages")]
    pub stages: Vec<String>,

    #[serde(default = "default_policy")]
    pub policy: VerificationPolicy,
}

impl Default for VerificationSection {
    fn default() -> Self {
        Self {
            stages: default_stages(),
            policy: default_policy(),
        }
    }
}

fn default_stages() -> Vec<String> {
    vec!["syntactic".into(), "test_runner".into(), "lsp_diag".into()]
}

fn default_policy() -> VerificationPolicy {
    VerificationPolicy::CollectAll
}

impl RuntimeConfig {

    pub fn load_from_workspace(root: &Path) -> Self {
        let path = root.join(".sen").join("config.toml");
        if !path.exists() {
            return Self::default();
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "inline_edit.runtime_config",
                    path = %path.display(),
                    error = %e,
                    "failed to read .sen/config.toml; using defaults",
                );
                return Self::default();
            }
        };
        match toml::from_str::<RuntimeConfig>(&raw) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(
                    target: "inline_edit.runtime_config",
                    path = %path.display(),
                    error = %e,
                    "failed to parse .sen/config.toml; using defaults",
                );
                Self::default()
            }
        }
    }
}

pub fn build_refiner_from_config(
    provider: Arc<dyn Provider>,
    model: impl Into<String>,
    cfg: &RefineSection,
) -> HttpLlmRefiner {
    HttpLlmRefiner::new(provider, model)
        .with_temperature(cfg.temperature)
        .with_timeout(Duration::from_secs(cfg.timeout_seconds))
        .with_max_recursive_attempts(cfg.max_recursive)
}

pub fn build_pipeline_from_config(
    root: &Path,
    lsp: Option<Arc<LspService>>,
    cfg: &VerificationSection,
) -> VerificationPipeline {
    let mut stages: Vec<Box<dyn Verifier>> = Vec::new();
    for name in &cfg.stages {
        match name.as_str() {
            "syntactic" => stages.push(Box::new(SyntacticVerifier::new())),
            "test_runner" => {
                let detected = TestRunnerBuilder::new(root.to_path_buf()).build();
                if detected.is_empty() {
                    stages.push(Box::new(
                        crate::agent::verification::test_runner::TestRunnerVerifier::dry_run(),
                    ));
                } else {
                    stages.extend(detected);
                }
            }
            "lsp_diag" => {
                if let Some(svc) = lsp.clone() {
                    let fetcher = LspPoolDiagnosticFetcher::new(svc, root.to_path_buf());
                    let verifier = LspDiagVerifier::new(Arc::new(fetcher));
                    stages.push(Box::new(verifier));
                }
            }
            other => {
                tracing::warn!(
                    target: "inline_edit.runtime_config",
                    stage = %other,
                    "unknown verification stage in [verification].stages; skipping",
                );
            }
        }
    }
    if stages.is_empty() {
        return VerificationPipeline::default_for_workspace(root, lsp);
    }
    VerificationPipeline::new(stages, cfg.policy.clone())
}
