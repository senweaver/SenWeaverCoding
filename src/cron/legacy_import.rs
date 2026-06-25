// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::Config;
use crate::cron::{AgentJobOptions, CronJobPatch, Schedule, SessionTarget};
use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct LegacyState {
    #[serde(default)]
    tasks: Vec<LegacyTask>,
}

#[derive(Debug, Deserialize)]
struct LegacyTask {
    prompt: String,
    #[serde(default)]
    priority: Option<String>,
    trigger: LegacyTrigger,
    #[serde(default)]
    max_duration_ms: Option<u64>,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    last_run_ms: Option<u64>,
    #[serde(default)]
    run_count: u32,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    name: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LegacyTrigger {
    Idle { after_idle_ms: u64 },
    Interval { every_ms: u64 },
    Once { at_ms: u64 },
    OnSessionEnd,
}

pub fn import_legacy_auto_dream(config: &Config, legacy_path: &Path) -> Result<usize> {
    if !legacy_path.exists() {
        return Ok(0);
    }

    let bytes = std::fs::read(legacy_path)
        .with_context(|| format!("Failed to read {}", legacy_path.display()))?;
    let state: LegacyState = serde_json::from_slice(&bytes)
        .with_context(|| format!("Failed to parse {}", legacy_path.display()))?;

    let now = Utc::now();
    let mut imported = 0usize;

    for task in state.tasks {
        let schedule = match task.trigger {
            LegacyTrigger::Idle { after_idle_ms } if after_idle_ms > 0 => {
                Schedule::Idle { after_idle_ms }
            }
            LegacyTrigger::Idle { .. } => continue,
            LegacyTrigger::Interval { every_ms } if every_ms > 0 => Schedule::Every { every_ms },
            LegacyTrigger::Interval { .. } => continue,
            LegacyTrigger::Once { at_ms } => {
                if task.run_count > 0 {
                    continue;
                }
                let at = Utc
                    .timestamp_millis_opt(i64::try_from(at_ms).unwrap_or(i64::MAX))
                    .single()
                    .unwrap_or(now);
                if at <= now {
                    continue;
                }
                Schedule::At { at }
            }
            LegacyTrigger::OnSessionEnd => Schedule::OnSessionEnd,
        };

        let allowed_tools = if task.allowed_tools.is_empty() {
            None
        } else {
            Some(task.allowed_tools.clone())
        };
        let name = task
            .name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| Some("Imported dream task".to_string()));

        let options = AgentJobOptions {
            session_target: SessionTarget::Isolated,
            allowed_tools,
            max_duration_ms: task.max_duration_ms,
            priority: task.priority.clone(),
            ..Default::default()
        };

        let created = match crate::cron::add_agent_job(config, name, schedule, &task.prompt, options)
        {
            Ok(job) => job,
            Err(e) => {
                tracing::warn!(error = %e, "skipping legacy auto_dream task during import");
                continue;
            }
        };

        let _ = task.last_run_ms;
        if !task.enabled {
            let _ = crate::cron::update_job(
                config,
                &created.id,
                CronJobPatch {
                    enabled: Some(false),
                    ..CronJobPatch::default()
                },
            );
        }

        imported += 1;
    }

    let migrated_path = legacy_path.with_extension("json.migrated");
    if let Err(e) = std::fs::rename(legacy_path, &migrated_path) {
        tracing::warn!(
            error = %e,
            from = %legacy_path.display(),
            to = %migrated_path.display(),
            "failed to archive legacy auto_dream.json after import"
        );
    }

    Ok(imported)
}
