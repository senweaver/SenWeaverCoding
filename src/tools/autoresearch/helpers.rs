// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::{self, Provider, ProviderRuntimeOptions};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use anyhow::{Context as _, Result, anyhow};
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinSet;

pub const REPORT_ROOT_DIRNAME: &str = ".senweavercoding/autoresearch";

#[derive(Clone)]
pub struct AutoresearchRuntime {
    pub security: Arc<SecurityPolicy>,
    pub provider_name: String,
    pub model: String,
    pub temperature: f64,
    pub api_key: Option<String>,
    pub provider_runtime_options: ProviderRuntimeOptions,
    pub workspace_root: Arc<RwLock<PathBuf>>,
}

impl AutoresearchRuntime {
    pub fn new(
        security: Arc<SecurityPolicy>,
        provider_name: String,
        model: String,
        temperature: f64,
        api_key: Option<String>,
        provider_runtime_options: ProviderRuntimeOptions,
        workspace_root: Arc<RwLock<PathBuf>>,
    ) -> Self {
        Self {
            security,
            provider_name,
            model,
            temperature,
            api_key,
            provider_runtime_options,
            workspace_root,
        }
    }

    pub fn enforce(&self, op: ToolOperation, tool: &str) -> Result<(), String> {
        self.security.enforce_tool_operation(op, tool)
    }

    pub fn workspace_dir(&self) -> PathBuf {
        self.workspace_root.read().clone()
    }

    pub async fn build_provider(&self) -> Result<Box<dyn Provider>, anyhow::Error> {
        providers::create_resilient_runtime_provider_async(
            self.provider_name.clone(),
            self.api_key.clone(),
            None,
            self.provider_runtime_options.clone(),
        )
        .await
        .map_err(|e| anyhow!("create provider failed: {e}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaTask {
    pub id: String,
    pub label: String,
    pub system_prompt: String,
    pub user_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaOutcome {
    pub id: String,
    pub label: String,
    pub raw_response: String,
    pub error: Option<String>,
    pub elapsed_ms: u128,
}

pub async fn fan_out_personas(
    runtime: &AutoresearchRuntime,
    tasks: Vec<PersonaTask>,
    model_override: Option<String>,
    temperature_override: Option<f64>,
) -> Vec<PersonaOutcome> {
    let provider_name = runtime.provider_name.clone();
    let api_key = runtime.api_key.clone();
    let runtime_options = runtime.provider_runtime_options.clone();
    let model = model_override.unwrap_or_else(|| runtime.model.clone());
    let temperature = temperature_override.unwrap_or(runtime.temperature);

    let mut join_set: JoinSet<PersonaOutcome> = JoinSet::new();
    for task in tasks {
        let provider_name = provider_name.clone();
        let api_key = api_key.clone();
        let runtime_options = runtime_options.clone();
        let model = model.clone();
        join_set.spawn(async move {
            let started = std::time::Instant::now();
            let provider_result = providers::create_resilient_runtime_provider_async(
                provider_name,
                api_key,
                None,
                runtime_options,
            )
            .await;
            let provider: Box<dyn Provider> = match provider_result {
                Ok(p) => p,
                Err(e) => {
                    return PersonaOutcome {
                        id: task.id.clone(),
                        label: task.label.clone(),
                        raw_response: String::new(),
                        error: Some(format!("create provider failed: {e}")),
                        elapsed_ms: started.elapsed().as_millis(),
                    };
                }
            };
            let response = provider
                .chat_with_system(
                    Some(&task.system_prompt),
                    &task.user_prompt,
                    &model,
                    temperature,
                )
                .await;
            let elapsed_ms = started.elapsed().as_millis();
            match response {
                Ok(text) => PersonaOutcome {
                    id: task.id,
                    label: task.label,
                    raw_response: text,
                    error: None,
                    elapsed_ms,
                },
                Err(e) => PersonaOutcome {
                    id: task.id,
                    label: task.label,
                    raw_response: String::new(),
                    error: Some(format!("provider call failed: {e:#}")),
                    elapsed_ms,
                },
            }
        });
    }

    let cancel_token = crate::providers::current_session_cancel_token();
    let mut outcomes = Vec::new();
    loop {
        let joined = match cancel_token.as_ref() {
            Some(token) => tokio::select! {
                biased;
                () = token.cancelled() => {
                    join_set.abort_all();
                    while join_set.join_next().await.is_some() {}
                    outcomes.push(PersonaOutcome {
                        id: "cancelled".to_string(),
                        label: "session cancelled".to_string(),
                        raw_response: String::new(),
                        error: Some("persona fan-out cancelled by session".to_string()),
                        elapsed_ms: 0,
                    });
                    return outcomes;
                }
                next = join_set.join_next() => match next {
                    Some(result) => result,
                    None => break,
                },
            },
            None => match join_set.join_next().await {
                Some(result) => result,
                None => break,
            },
        };
        match joined {
            Ok(outcome) => outcomes.push(outcome),
            Err(join_err) => outcomes.push(PersonaOutcome {
                id: "unknown".to_string(),
                label: "task panicked".to_string(),
                raw_response: String::new(),
                error: Some(format!("task join error: {join_err}")),
                elapsed_ms: 0,
            }),
        }
    }
    outcomes
}

pub fn extract_json_block(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(end) = rest.rfind("```") {
            return Some(rest[..end].trim());
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(end) = rest.rfind("```") {
            return Some(rest[..end].trim());
        }
    }
    let start = trimmed.find(['{', '['])?;
    let last_close = trimmed.rfind(['}', ']'])?;
    if last_close > start {
        Some(&trimmed[start..=last_close])
    } else {
        None
    }
}

pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("session");
    }
    out
}

pub fn timestamp_slug() -> String {
    Utc::now().format("%Y%m%d-%H%M%S").to_string()
}

pub fn ensure_report_dir(
    workspace_root: &Path,
    family: &str,
    slug_hint: Option<&str>,
) -> Result<PathBuf, std::io::Error> {
    let root = workspace_root.join(REPORT_ROOT_DIRNAME).join(family);
    std::fs::create_dir_all(&root)?;
    let mut name = format!("{}-{}", family, timestamp_slug());
    if let Some(hint) = slug_hint {
        let hint = slugify(hint);
        if !hint.is_empty() {
            name = format!("{}-{}-{}", family, timestamp_slug(), hint);
        }
    }
    let dir = root.join(name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn write_text(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeSample {
    pub path: String,
    pub byte_len: usize,
    pub head: String,
    pub truncated: bool,
}

pub fn collect_scope_samples(
    workspace_root: &Path,
    raw_scope: &[String],
    per_file_max_bytes: usize,
    overall_file_cap: usize,
    security: &SecurityPolicy,
) -> Result<Vec<ScopeSample>, anyhow::Error> {
    let mut visited: BTreeSet<PathBuf> = BTreeSet::new();
    let mut samples: Vec<ScopeSample> = Vec::new();
    for spec in raw_scope {
        if samples.len() >= overall_file_cap {
            break;
        }
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            continue;
        }
        let direct = workspace_root.join(trimmed);
        if direct.is_file() {
            push_file_sample(
                &direct,
                workspace_root,
                per_file_max_bytes,
                &mut visited,
                &mut samples,
                security,
            )?;
            continue;
        }
        if direct.is_dir() {
            for entry in walk_dir(&direct, overall_file_cap.saturating_sub(samples.len())) {
                if samples.len() >= overall_file_cap {
                    break;
                }
                push_file_sample(
                    &entry,
                    workspace_root,
                    per_file_max_bytes,
                    &mut visited,
                    &mut samples,
                    security,
                )?;
            }
            continue;
        }
        for entry in glob_matches(workspace_root, trimmed)? {
            if samples.len() >= overall_file_cap {
                break;
            }
            push_file_sample(
                &entry,
                workspace_root,
                per_file_max_bytes,
                &mut visited,
                &mut samples,
                security,
            )?;
        }
    }
    Ok(samples)
}

fn push_file_sample(
    path: &Path,
    workspace_root: &Path,
    per_file_max_bytes: usize,
    visited: &mut BTreeSet<PathBuf>,
    out: &mut Vec<ScopeSample>,
    security: &SecurityPolicy,
) -> Result<(), anyhow::Error> {
    let Ok(canonical) = path.canonicalize() else {
        return Ok(());
    };
    if !security.is_resolved_path_allowed(&canonical) {
        return Ok(());
    }
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }
    if !canonical.is_file() {
        return Ok(());
    }
    let bytes = std::fs::read(&canonical)
        .with_context(|| format!("read {}", canonical.display()))?;
    let byte_len = bytes.len();
    let truncated = byte_len > per_file_max_bytes;
    let cap = per_file_max_bytes.min(byte_len);
    let head_bytes = &bytes[..cap];
    let head = match std::str::from_utf8(head_bytes) {
        Ok(s) => s.to_string(),
        Err(_) => return Ok(()),
    };
    let rel = crate::util::path_relative_to(&canonical, workspace_root)
        .unwrap_or_else(|| canonical.clone())
        .display()
        .to_string()
        .replace('\\', "/");
    out.push(ScopeSample {
        path: rel,
        byte_len,
        head,
        truncated,
    });
    Ok(())
}

fn walk_dir(root: &Path, cap: usize) -> Vec<PathBuf> {
    let mut acc: Vec<PathBuf> = Vec::new();
    let mut visited_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    if let Ok(canonical_root) = root.canonicalize() {
        visited_dirs.insert(canonical_root);
    }
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if acc.len() >= cap {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if acc.len() >= cap {
                break;
            }
            let path = entry.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if should_skip_entry(name) {
                continue;
            }
            if path.is_dir() {
                let Ok(canonical) = path.canonicalize() else {
                    continue;
                };
                if !visited_dirs.insert(canonical) {
                    continue;
                }
                stack.push(path);
            } else if path.is_file() {
                acc.push(path);
            }
        }
    }
    acc
}

fn should_skip_entry(name: &str) -> bool {
    matches!(
        name,
        "target"
            | "node_modules"
            | "dist"
            | "build"
            | ".git"
            | ".idea"
            | ".cargo"
            | ".venv"
            | "venv"
            | "__pycache__"
    )
}

fn glob_matches(workspace_root: &Path, pattern: &str) -> Result<Vec<PathBuf>, anyhow::Error> {
    let pattern_path = workspace_root.join(pattern);
    let pattern_str = pattern_path.to_string_lossy().to_string();
    let mut out = Vec::new();
    match glob::glob(&pattern_str) {
        Ok(paths) => {
            for path in paths.flatten() {
                if path.is_file() {
                    out.push(path);
                }
            }
        }
        Err(_) => {
            return Ok(out);
        }
    }
    Ok(out)
}

pub fn build_scope_context_snippet(samples: &[ScopeSample], max_chars: usize) -> String {
    let mut buf = String::new();
    for sample in samples {
        if buf.len() >= max_chars {
            break;
        }
        let remain = max_chars.saturating_sub(buf.len());
        let head = if sample.head.len() > remain {
            &sample.head[..crate::util::floor_char_boundary(&sample.head, remain)]
        } else {
            &sample.head[..]
        };
        buf.push_str("===== FILE: ");
        buf.push_str(&sample.path);
        if sample.truncated {
            buf.push_str(" (truncated)");
        }
        buf.push_str(" =====\n");
        buf.push_str(head);
        buf.push_str("\n\n");
    }
    if buf.is_empty() {
        buf.push_str("<no scope samples collected>");
    }
    buf
}

pub fn severity_rank(severity: &str) -> u8 {
    match severity.to_ascii_lowercase().as_str() {
        "critical" => 5,
        "high" => 4,
        "medium" | "moderate" => 3,
        "low" => 2,
        "info" | "informational" => 1,
        _ => 0,
    }
}

pub fn parse_severity(value: &str) -> String {
    let lowered = value.trim().to_ascii_lowercase();
    if ["critical", "high", "medium", "low", "info"].contains(&lowered.as_str()) {
        lowered
    } else if lowered == "moderate" {
        "medium".to_string()
    } else if lowered == "informational" {
        "info".to_string()
    } else {
        "medium".to_string()
    }
}

pub fn render_envelope(family: &str, body: &str) -> String {
    let begin = format!("==={}_REPORT_BEGIN===", family.to_ascii_uppercase());
    let end = format!("==={}_REPORT_END===", family.to_ascii_uppercase());
    let body = if crate::token_saver::is_enabled() {
        crate::token_saver::compact_tool_output(
            &format!("autoresearch_{family}"),
            body,
            &crate::token_saver::global(),
        )
    } else {
        body.to_string()
    };
    format!("{begin}\n{body}\n{end}")
}
