// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::ObservabilityConfig;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};
use uuid::Uuid;

const DEFAULT_TRACE_REL_PATH: &str = "state/runtime-trace.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTraceStorageMode {
    None,
    Rolling,
    Full,
}

impl RuntimeTraceStorageMode {
    fn from_raw(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "rolling" => Self::Rolling,
            "full" => Self::Full,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeTraceEvent {
    pub id: String,
    pub timestamp: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

struct RuntimeTraceLogger {
    mode: RuntimeTraceStorageMode,
    max_entries: usize,
    path: PathBuf,
    write_lock: std::sync::Mutex<()>,
}

impl RuntimeTraceLogger {
    fn new(mode: RuntimeTraceStorageMode, max_entries: usize, path: PathBuf) -> Self {
        Self {
            mode,
            max_entries: max_entries.max(1),
            path,
            write_lock: std::sync::Mutex::new(()),
        }
    }

    fn append(&self, event: &RuntimeTraceEvent) -> Result<()> {
        if self.mode == RuntimeTraceStorageMode::None {
            return Ok(());
        }

        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let line = serde_json::to_string(event)?;
        let mut options = OpenOptions::new();
        options.create(true).append(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options.open(&self.path)?;
        writeln!(file, "{line}")?;
        file.sync_data()?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }

        if self.mode == RuntimeTraceStorageMode::Rolling {
            self.trim_to_last_entries()?;
        }

        Ok(())
    }

    fn trim_to_last_entries(&self) -> Result<()> {
        let raw = fs::read_to_string(&self.path).unwrap_or_default();
        let lines: Vec<&str> = raw
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();

        if lines.len() <= self.max_entries {
            return Ok(());
        }

        let keep_from = lines.len().saturating_sub(self.max_entries);
        let kept = &lines[keep_from..];
        let mut rewritten = kept.join("\n");
        rewritten.push('\n');

        let tmp = self.path.with_extension(format!(
            "tmp.{}.{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::write(&tmp, rewritten)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }

        fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

static TRACE_LOGGER: LazyLock<RwLock<Option<Arc<RuntimeTraceLogger>>>> =
    LazyLock::new(|| RwLock::new(None));

pub fn storage_mode_from_config(config: &ObservabilityConfig) -> RuntimeTraceStorageMode {
    let mode = RuntimeTraceStorageMode::from_raw(&config.runtime_trace_mode);
    if mode == RuntimeTraceStorageMode::None
        && !config.runtime_trace_mode.trim().is_empty()
        && !config.runtime_trace_mode.eq_ignore_ascii_case("none")
    {
        tracing::warn!(
            mode = %config.runtime_trace_mode,
            "Unknown observability.runtime_trace_mode; falling back to none"
        );
    }
    mode
}

pub fn resolve_trace_path(config: &ObservabilityConfig, workspace_dir: &Path) -> PathBuf {
    let raw = config.runtime_trace_path.trim();
    let fallback = workspace_dir.join(DEFAULT_TRACE_REL_PATH);
    if raw.is_empty() {
        return fallback;
    }

    let configured = PathBuf::from(raw);
    if configured.is_absolute() {
        configured
    } else {
        workspace_dir.join(configured)
    }
}

pub fn init_from_config(config: &ObservabilityConfig, workspace_dir: &Path) {
    let mode = storage_mode_from_config(config);
    let logger = if mode == RuntimeTraceStorageMode::None {
        None
    } else {
        Some(Arc::new(RuntimeTraceLogger::new(
            mode,
            config.runtime_trace_max_entries.max(1),
            resolve_trace_path(config, workspace_dir),
        )))
    };

    let mut guard = TRACE_LOGGER.write().unwrap_or_else(|e| e.into_inner());
    *guard = logger;
}

#[derive(Debug, Clone, Default)]
pub struct AgentSpanContext<'a> {
    pub parent_agent_id: Option<&'a str>,
    pub agent_id: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub delegation_id: Option<&'a str>,
}

pub fn record_event(
    event_type: &str,
    channel: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
    turn_id: Option<&str>,
    success: Option<bool>,
    message: Option<&str>,
    payload: Value,
) {
    record_event_with_ctx(
        event_type,
        channel,
        provider,
        model,
        turn_id,
        success,
        message,
        payload,
        AgentSpanContext::default(),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn record_event_with_ctx(
    event_type: &str,
    channel: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
    turn_id: Option<&str>,
    success: Option<bool>,
    message: Option<&str>,
    payload: Value,
    ctx: AgentSpanContext<'_>,
) {
    let logger = TRACE_LOGGER
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(logger) = logger else {
        return;
    };

    let event = RuntimeTraceEvent {
        id: Uuid::new_v4().to_string(),
        timestamp: Utc::now().to_rfc3339(),
        event_type: event_type.to_string(),
        channel: channel.map(str::to_string),
        provider: provider.map(str::to_string),
        model: model.map(str::to_string),
        turn_id: turn_id.map(str::to_string),
        parent_agent_id: ctx.parent_agent_id.map(str::to_string),
        agent_id: ctx.agent_id.map(str::to_string),
        task_id: ctx.task_id.map(str::to_string),
        delegation_id: ctx.delegation_id.map(str::to_string),
        success,
        message: message.map(str::to_string),
        payload,
    };

    if let Err(err) = logger.append(&event) {
        tracing::warn!("Failed to write runtime trace event: {err}");
    }
}

pub fn load_events(
    path: &Path,
    limit: usize,
    event_filter: Option<&str>,
    contains: Option<&str>,
) -> Result<Vec<RuntimeTraceEvent>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(path)?;
    let mut events = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match serde_json::from_str::<RuntimeTraceEvent>(trimmed) {
            Ok(event) => events.push(event),
            Err(err) => tracing::warn!("Skipping malformed runtime trace line: {err}"),
        }
    }

    if let Some(filter) = event_filter.map(str::trim).filter(|f| !f.is_empty()) {
        let normalized = filter.to_ascii_lowercase();
        events.retain(|event| event.event_type.to_ascii_lowercase() == normalized);
    }

    if let Some(needle) = contains.map(str::trim).filter(|s| !s.is_empty()) {
        let needle = needle.to_ascii_lowercase();
        events.retain(|event| {
            let mut haystack = format!(
                "{} {} {}",
                event.event_type,
                event.message.as_deref().unwrap_or_default(),
                event.payload
            );
            if let Some(channel) = &event.channel {
                haystack.push_str(channel);
            }
            if let Some(provider) = &event.provider {
                haystack.push_str(provider);
            }
            if let Some(model) = &event.model {
                haystack.push_str(model);
            }
            haystack.to_ascii_lowercase().contains(&needle)
        });
    }

    if events.len() > limit {
        let keep_from = events.len() - limit;
        events = events.split_off(keep_from);
    }

    events.reverse();
    Ok(events)
}

pub fn find_event_by_id(path: &Path, id: &str) -> Result<Option<RuntimeTraceEvent>> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)?;
    for line in raw.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<RuntimeTraceEvent>(trimmed) {
            if event.id == id {
                return Ok(Some(event));
            }
        }
    }

    Ok(None)
}
