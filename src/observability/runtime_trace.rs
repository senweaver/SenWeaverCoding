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

const RUNTIME_TRACE_CHANNEL_CAP: usize = 8192;

struct RuntimeTraceLogger {
    tx: Option<std::sync::mpsc::SyncSender<RuntimeTraceEvent>>,
}

impl RuntimeTraceLogger {
    fn new(mode: RuntimeTraceStorageMode, max_entries: usize, path: PathBuf) -> Self {
        if mode == RuntimeTraceStorageMode::None {
            return Self { tx: None };
        }
        let max_entries = max_entries.max(1);
        let (tx, rx) =
            std::sync::mpsc::sync_channel::<RuntimeTraceEvent>(RUNTIME_TRACE_CHANNEL_CAP);
        let spawned = std::thread::Builder::new()
            .name("runtime-trace".to_string())
            .spawn(move || trace_writer_loop(mode, max_entries, path, rx))
            .ok();
        Self {
            tx: spawned.map(|_| tx),
        }
    }

    fn submit(&self, event: RuntimeTraceEvent) {
        if let Some(tx) = &self.tx {
            match tx.try_send(event) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    tracing::debug!(
                        "runtime trace channel saturated; dropping trace event to avoid unbounded growth"
                    );
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    tracing::warn!("runtime trace writer thread is gone; trace event dropped");
                }
            }
        }
    }
}

fn trace_writer_loop(
    mode: RuntimeTraceStorageMode,
    max_entries: usize,
    path: PathBuf,
    rx: std::sync::mpsc::Receiver<RuntimeTraceEvent>,
) {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    const BATCH_MAX: usize = 64;
    const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

    let mut batch: Vec<RuntimeTraceEvent> = Vec::new();
    let mut last_flush = Instant::now();

    loop {
        let wait = FLUSH_INTERVAL.saturating_sub(last_flush.elapsed());
        match rx.recv_timeout(wait) {
            Ok(event) => {
                batch.push(event);
                if batch.len() >= BATCH_MAX {
                    flush_trace_batch(mode, max_entries, &path, &mut batch);
                    last_flush = Instant::now();
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !batch.is_empty() {
                    flush_trace_batch(mode, max_entries, &path, &mut batch);
                }
                last_flush = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush_trace_batch(mode, max_entries, &path, &mut batch);
                break;
            }
        }

        if !batch.is_empty() && last_flush.elapsed() >= FLUSH_INTERVAL {
            flush_trace_batch(mode, max_entries, &path, &mut batch);
            last_flush = Instant::now();
        }
    }
}

fn flush_trace_batch(
    mode: RuntimeTraceStorageMode,
    max_entries: usize,
    path: &Path,
    batch: &mut Vec<RuntimeTraceEvent>,
) {
    if batch.is_empty() {
        return;
    }
    if let Err(err) = write_trace_batch(mode, max_entries, path, batch) {
        tracing::warn!("Failed to write runtime trace batch: {err}");
    }
    batch.clear();
}

fn write_trace_batch(
    mode: RuntimeTraceStorageMode,
    max_entries: usize,
    path: &Path,
    batch: &[RuntimeTraceEvent],
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut options = OpenOptions::new();
    options.create(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let file = options.open(path)?;
    let mut writer = std::io::BufWriter::new(file);
    for event in batch {
        let line = serde_json::to_string(event)?;
        writeln!(writer, "{line}")?;
    }
    writer.flush()?;
    let file = writer
        .into_inner()
        .map_err(|e| anyhow::anyhow!("failed to flush runtime trace buffer: {e}"))?;
    file.sync_data()?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }

    if mode == RuntimeTraceStorageMode::Rolling {
        trim_to_last_entries(path, max_entries)?;
    }

    Ok(())
}

fn trim_to_last_entries(path: &Path, max_entries: usize) -> Result<()> {
    let raw = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if lines.len() <= max_entries {
        return Ok(());
    }

    let keep_from = lines.len().saturating_sub(max_entries);
    let kept = &lines[keep_from..];
    let mut rewritten = kept.join("\n");
    rewritten.push('\n');

    let tmp = path.with_extension(format!(
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

    fs::rename(tmp, path)?;
    Ok(())
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

pub fn is_enabled() -> bool {
    TRACE_LOGGER
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
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

    logger.submit(event);
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
