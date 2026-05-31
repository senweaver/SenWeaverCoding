// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod openai;
pub mod anthropic_messages;
pub mod hf_trl_dpo;
pub mod rl_sar;
pub mod agent_trajectory;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;

use super::EvolutionEngine;
use super::types::{
    EvolutionExportConfig, EvolutionExportFormat, ExportRecord, TurnRecord,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportFilter {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub coding_mode: Option<String>,
    #[serde(default)]
    pub start_ms: Option<i64>,
    #[serde(default)]
    pub end_ms: Option<i64>,
    #[serde(default)]
    pub min_reward: Option<f32>,
    #[serde(default)]
    pub max_samples: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportOptions {
    #[serde(default)]
    pub redact_workspace_paths: Option<bool>,
    #[serde(default)]
    pub redact_secrets: Option<bool>,
    #[serde(default)]
    pub include_thinking: Option<bool>,
}

pub struct ExportPreview {
    pub format: EvolutionExportFormat,
    pub samples: Vec<serde_json::Value>,
    pub total_eligible: u64,
}

pub fn export_to_file(
    engine: &Arc<EvolutionEngine>,
    format: EvolutionExportFormat,
    filter: &ExportFilter,
    options: &ExportOptions,
) -> Result<ExportRecord> {
    if !engine.persist_training_data() {
        return Err(anyhow!("persistence_disabled"));
    }
    let snapshot = engine.config_snapshot();
    let store = engine.store();
    let exports_dir = effective_export_dir(store.exports_dir().to_path_buf(), &snapshot.export);
    fs::create_dir_all(&exports_dir).with_context(|| {
        format!("failed to create export dir {}", exports_dir.display())
    })?;
    let id = format!("export_{}", uuid::Uuid::new_v4().simple());
    let file_name = format!("{}_{}.jsonl", id, format.as_str());
    let path = exports_dir.join(&file_name);
    let file = File::create(&path)
        .with_context(|| format!("failed to create export file {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut sample_count: u64 = 0;
    let mut window_start: Option<DateTime<Utc>> = None;
    let mut window_end: Option<DateTime<Utc>> = None;

    let mut total_size: u64 = 0;
    let processor = make_processor(format);
    iter_turns_filtered(
        store.turns_path().to_path_buf(),
        filter,
        |turn| {
            if let Some(value) = processor(&turn, options, &snapshot.export) {
                let line = serde_json::to_string(&value)
                    .context("failed to serialise export sample")?;
                writer
                    .write_all(line.as_bytes())
                    .context("failed to write export sample")?;
                writer.write_all(b"\n").ok();
                hasher.update(line.as_bytes());
                hasher.update(b"\n");
                sample_count += 1;
                total_size += line.len() as u64 + 1;
                if window_start.is_none() || Some(turn.ts) < window_start {
                    window_start = Some(turn.ts);
                }
                let end_ts = turn.completed_ts.unwrap_or(turn.ts);
                if window_end.is_none() || Some(end_ts) > window_end {
                    window_end = Some(end_ts);
                }
            }
            Ok(())
        },
    )?;
    writer.flush().ok();
    drop(writer);

    let md5 = format!("{:x}", hasher.finalize());
    let record = ExportRecord {
        id,
        format,
        path: path.to_string_lossy().to_string(),
        sample_count,
        size_bytes: total_size,
        md5,
        time_window_start: window_start,
        time_window_end: window_end,
        created_at: Utc::now(),
    };
    store.upsert_export(&record)?;
    trigger_auto_push(engine, &record);
    Ok(record)
}

fn trigger_auto_push(engine: &Arc<EvolutionEngine>, record: &ExportRecord) {
    let targets = match engine.store().list_cloud_targets() {
        Ok(items) => items,
        Err(error) => {
            tracing::debug!(error = %error, "auto-push: list_cloud_targets failed");
            return;
        }
    };
    let now = Utc::now();
    for target in targets {
        if !target.enabled || !target.auto_push {
            continue;
        }
        if target.default_format != record.format {
            continue;
        }
        if u64::from(target.auto_push_min_samples) > record.sample_count {
            continue;
        }
        if let Some(last) = target.last_pushed_at {
            let interval_ms = i64::from(target.auto_push_min_interval_hours) * 3_600_000;
            if interval_ms > 0
                && now.timestamp_millis().saturating_sub(last.timestamp_millis()) < interval_ms
            {
                continue;
            }
        }
        let engine_clone = Arc::clone(engine);
        let target_id = target.id.clone();
        let export_id = record.id.clone();
        crate::runtime::spawn_supervised("evolution.auto_push_export", async move {
            match super::push_export_to_target(&engine_clone, &target_id, &export_id).await {
                Ok(receipt) => tracing::info!(
                    target_id = %target_id,
                    export_id = %export_id,
                    status = %receipt.status,
                    "evolution: auto-push completed"
                ),
                Err(error) => tracing::warn!(
                    target_id = %target_id,
                    export_id = %export_id,
                    error = %error,
                    "evolution: auto-push failed"
                ),
            }
        });
    }
}

pub fn preview_export(
    engine: &Arc<EvolutionEngine>,
    format: EvolutionExportFormat,
    filter: &ExportFilter,
    options: &ExportOptions,
    max_samples: usize,
) -> Result<ExportPreview> {
    if !engine.persist_training_data() {
        return Err(anyhow!("persistence_disabled"));
    }
    let snapshot = engine.config_snapshot();
    let store = engine.store();
    let mut samples: Vec<serde_json::Value> = Vec::new();
    let mut total_eligible: u64 = 0;
    let processor = make_processor(format);
    iter_turns_filtered(
        store.turns_path().to_path_buf(),
        filter,
        |turn| {
            if let Some(value) = processor(&turn, options, &snapshot.export) {
                if samples.len() < max_samples {
                    samples.push(value);
                }
                total_eligible += 1;
            }
            Ok(())
        },
    )?;
    Ok(ExportPreview {
        format,
        samples,
        total_eligible,
    })
}

type ProjectFn = Box<dyn Fn(&TurnRecord, &ExportOptions, &EvolutionExportConfig) -> Option<serde_json::Value>>;

fn make_processor(format: EvolutionExportFormat) -> ProjectFn {
    match format {
        EvolutionExportFormat::OpenaiSft => Box::new(openai::sft::project),
        EvolutionExportFormat::OpenaiDpo => Box::new(openai::dpo::project),
        EvolutionExportFormat::AnthropicMessages => Box::new(anthropic_messages::project),
        EvolutionExportFormat::HfTrlDpo => Box::new(hf_trl_dpo::project),
        EvolutionExportFormat::RlSar => Box::new(rl_sar::project),
        EvolutionExportFormat::AgentTrajectory => Box::new(agent_trajectory::project),
    }
}

fn effective_export_dir(default_dir: PathBuf, cfg: &EvolutionExportConfig) -> PathBuf {
    cfg.export_dir.clone().unwrap_or(default_dir)
}

pub(crate) fn iter_turns_filtered<F>(
    turns_path: PathBuf,
    filter: &ExportFilter,
    mut on_turn: F,
) -> Result<()>
where
    F: FnMut(TurnRecord) -> Result<()>,
{
    if !turns_path.exists() {
        return Ok(());
    }
    let file = OpenOptions::new()
        .read(true)
        .open(&turns_path)
        .with_context(|| format!("failed to open turns file {}", turns_path.display()))?;
    let reader = BufReader::new(file);
    let mut emitted: u64 = 0;
    for line in reader.lines() {
        let line = line.context("failed to read turns line")?;
        if line.trim().is_empty() {
            continue;
        }
        let turn: TurnRecord = match serde_json::from_str(&line) {
            Ok(t) => t,
            Err(error) => {
                tracing::debug!(error = %error, "skipping malformed turn line");
                continue;
            }
        };
        if !matches_filter(&turn, filter) {
            continue;
        }
        on_turn(turn)?;
        emitted += 1;
        if let Some(max) = filter.max_samples {
            if emitted >= max {
                break;
            }
        }
    }
    Ok(())
}

fn matches_filter(turn: &TurnRecord, filter: &ExportFilter) -> bool {
    if let Some(ref sid) = filter.session_id {
        if turn.session_id != *sid {
            return false;
        }
    }
    if let Some(ref mode) = filter.coding_mode {
        match &turn.coding_mode {
            Some(active) if active.eq_ignore_ascii_case(mode) => {}
            _ => return false,
        }
    }
    if let Some(start) = filter.start_ms {
        if turn.ts.timestamp_millis() < start {
            return false;
        }
    }
    if let Some(end) = filter.end_ms {
        let ts = turn.completed_ts.unwrap_or(turn.ts).timestamp_millis();
        if ts > end {
            return false;
        }
    }
    if let Some(min) = filter.min_reward {
        if turn.reward.final_score < min {
            return false;
        }
    }
    true
}

pub(crate) fn redact_text(text: &str, options: &ExportOptions, cfg: &EvolutionExportConfig) -> String {
    let mut out = text.to_string();
    let redact_paths = options.redact_workspace_paths.unwrap_or(cfg.redact_workspace_paths);
    let redact_secrets = options.redact_secrets.unwrap_or(cfg.redact_secrets);
    if redact_paths {
        out = redact_workspace_paths(&out);
    }
    if redact_secrets {
        out = redact_secrets_in(&out);
    }
    out
}

fn redact_workspace_paths(text: &str) -> String {
    let mut out = text.replace('\\', "/");
    let home_candidates = [
        std::env::var("HOME").ok(),
        std::env::var("USERPROFILE").ok(),
    ];
    for candidate in home_candidates.iter().flatten() {
        let normalized = candidate.replace('\\', "/");
        if !normalized.is_empty() {
            out = out.replace(&normalized, "<HOME>");
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_str = cwd.to_string_lossy().replace('\\', "/");
        if !cwd_str.is_empty() {
            out = out.replace(&cwd_str, "<WORKSPACE>");
        }
    }
    out
}

fn redact_secrets_in(text: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock as StdOnceLock;
    static PATTERNS: StdOnceLock<Vec<Regex>> = StdOnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)\b(api[_-]?key|secret|token|password|bearer)\s*[:=]\s*[A-Za-z0-9_\-/+=]{8,}")
                .expect("api_key regex"),
            Regex::new(r"sk-[A-Za-z0-9]{20,}").expect("openai key regex"),
            Regex::new(r"hf_[A-Za-z0-9]{20,}").expect("hf token regex"),
            Regex::new(r"ghp_[A-Za-z0-9]{20,}").expect("github token regex"),
            Regex::new(r"AIza[A-Za-z0-9_-]{30,}").expect("google api key regex"),
            Regex::new(r"AKIA[A-Z0-9]{16}").expect("aws access key regex"),
        ]
    });
    let mut out = text.to_string();
    for pat in patterns {
        out = pat.replace_all(&out, "<REDACTED>").into_owned();
    }
    out
}
