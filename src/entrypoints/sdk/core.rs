// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;

pub type SdkSessionHandle = Arc<Mutex<Option<SdkSession>>>;

use anyhow::Result;
use tokio::sync::{Mutex, mpsc};

use crate::agent::{Agent, TurnEvent};
use crate::config::Config;

use super::types::{
    SdkConfig, SdkHookCallback, SdkMessage, SdkStatus, SdkToolCall, SdkTurnEvent,
};

#[derive(Default, Clone)]
struct SdkUsageAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    request_count: u64,
}

#[derive(Default, Clone, Copy)]
struct GlobalUsageSnapshot {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    request_count: u64,
}

fn snapshot_global_usage() -> GlobalUsageSnapshot {
    crate::bootstrap::try_get_state()
        .map(|bs| {
            bs.read(|state| {
                let mut snapshot = GlobalUsageSnapshot::default();
                for usage in state.model_usage.values() {
                    snapshot.input_tokens += usage.input_tokens;
                    snapshot.output_tokens += usage.output_tokens;
                    snapshot.cache_read_tokens += usage.cache_read_input_tokens;
                    snapshot.request_count += usage.request_count;
                }
                snapshot
            })
        })
        .unwrap_or_default()
}

fn hook_payload_for_event(
    event: &TurnEvent,
) -> Option<(
    crate::entrypoints::sdk::types::HookEvent,
    serde_json::Value,
)> {
    use crate::entrypoints::sdk::types::HookEvent;
    match event {
        TurnEvent::ToolCall {
            name,
            args,
            tool_call_id,
        } => Some((
            HookEvent::PreToolUse,
            serde_json::json!({
                "tool_name": name,
                "input": args,
                "tool_call_id": tool_call_id,
            }),
        )),
        TurnEvent::ToolResult {
            name,
            output,
            success,
            tool_call_id,
        } => Some((
            HookEvent::PostToolUse,
            serde_json::json!({
                "tool_name": name,
                "output": output,
                "success": success,
                "tool_call_id": tool_call_id,
            }),
        )),
        TurnEvent::Error { message } => Some((
            HookEvent::Notification,
            serde_json::json!({
                "kind": "error",
                "message": message,
            }),
        )),
        TurnEvent::PermissionRequest {
            request_id,
            tool_name,
            ..
        } => Some((
            HookEvent::Notification,
            serde_json::json!({
                "kind": "permission_request",
                "request_id": request_id,
                "tool_name": tool_name,
            }),
        )),
        TurnEvent::WorkerCompleted {
            worker_id,
            success,
            summary,
        } => Some((
            HookEvent::SubagentStop,
            serde_json::json!({
                "worker_id": worker_id,
                "success": success,
                "summary": summary,
            }),
        )),
        TurnEvent::WorkerStopped { worker_id, reason } => Some((
            HookEvent::SubagentStop,
            serde_json::json!({
                "worker_id": worker_id,
                "success": false,
                "reason": reason,
            }),
        )),
        _ => None,
    }
}

pub struct SdkSession {

    sdk_config: SdkConfig,

    agent: Arc<Mutex<Option<Agent>>>,

    usage: Arc<Mutex<SdkUsageAccumulator>>,

    hooks: Arc<Mutex<HashMap<crate::entrypoints::sdk::types::HookEvent, SdkHookCallback>>>,

    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl SdkSession {

    pub async fn send_message(&self, message: SdkMessage) -> Result<SdkMessage> {
        let mut guard = self.agent.lock().await;
        let agent = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no active session  -  call start_session() first"))?;

        let start = std::time::Instant::now();
        let usage_before = snapshot_global_usage();
        let response_text = agent.turn(&message.content).await?;
        self.accumulate_turn_usage(usage_before).await;
        self.fire_stop_hook(&response_text).await;
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let metadata = self.build_metadata(duration_ms);

        Ok(SdkMessage {
            role: "assistant".to_string(),
            content: response_text,
            tool_calls: Vec::new(),
            metadata: Some(metadata),
        })
    }

    pub async fn send_message_streamed(
        &self,
        message: &SdkMessage,
        event_tx: mpsc::Sender<SdkTurnEvent>,
    ) -> Result<SdkMessage> {
        let mut guard = self.agent.lock().await;
        let agent = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no active session  -  call start_session() first"))?;

        let start = std::time::Instant::now();
        let usage_before = snapshot_global_usage();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);

        let sdk_tx = event_tx.clone();
        let hooks = Arc::clone(&self.hooks);
        let relay_task =
            crate::runtime::spawn_supervised("entrypoints.sdk.event_relay", async move {
                while let Some(event) = rx.recv().await {
                    let mut forward = true;
                    if let Some((hook_event, payload)) = hook_payload_for_event(&event) {
                        let hooks_guard = hooks.lock().await;
                        if let Some(callback) = hooks_guard.get(&hook_event) {
                            forward = callback(hook_event, payload);
                        }
                    }
                    if !forward {
                        continue;
                    }
                    if matches!(event, crate::agent::TurnEvent::ToolArgsDelta { .. }) {
                        continue;
                    }
                    let sdk_event: SdkTurnEvent = event.into();
                    if sdk_tx.send(sdk_event).await.is_err() {

                        break;
                    }
                }
            });

        let turn_result = {
            use futures_util::FutureExt as _;
            std::panic::AssertUnwindSafe(agent.turn_streamed(&message.content, tx))
                .catch_unwind()
                .await
        };

        let _ = relay_task.into_inner().await;

        let response_text = match turn_result {
            Ok(inner) => inner?,
            Err(panic) => {
                return Err(anyhow::anyhow!(
                    "internal error recovered: {}",
                    crate::util::describe_panic(&*panic)
                ));
            }
        };

        self.accumulate_turn_usage(usage_before).await;
        self.fire_stop_hook(&response_text).await;

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let metadata = self.build_metadata(duration_ms);

        Ok(SdkMessage {
            role: "assistant".to_string(),
            content: response_text,
            tool_calls: Vec::new(),
            metadata: Some(metadata),
        })
    }

    async fn accumulate_turn_usage(&self, before: GlobalUsageSnapshot) {
        let after = snapshot_global_usage();
        let mut acc = self.usage.lock().await;
        acc.input_tokens += after.input_tokens.saturating_sub(before.input_tokens);
        acc.output_tokens += after.output_tokens.saturating_sub(before.output_tokens);
        acc.cache_read_tokens += after
            .cache_read_tokens
            .saturating_sub(before.cache_read_tokens);
        acc.request_count += after
            .request_count
            .saturating_sub(before.request_count)
            .max(1);
    }

    async fn fire_stop_hook(&self, response_text: &str) {
        use crate::entrypoints::sdk::types::HookEvent;
        let hooks_guard = self.hooks.lock().await;
        if let Some(callback) = hooks_guard.get(&HookEvent::Stop) {
            let _ = callback(
                HookEvent::Stop,
                serde_json::json!({
                    "response_chars": response_text.len(),
                }),
            );
        }
    }

    pub fn status(&self) -> SdkStatus {
        if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
            return SdkStatus::Stopped;
        }
        SdkStatus::Running
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let mut guard = self.agent.lock().await;
        *guard = None;

        tracing::info!("SDK session stopped");
        Ok(())
    }

    pub fn session_usage(&self) -> crate::entrypoints::sdk::types::SdkModelUsage {
        let acc = self
            .usage
            .try_lock()
            .map(|guard| (*guard).clone())
            .unwrap_or_default();
        crate::entrypoints::sdk::types::SdkModelUsage {
            model: self.sdk_config.model.clone().unwrap_or_default(),
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: acc.cache_read_tokens,
            total_cost_usd: 0.0,
            request_count: acc.request_count,
        }
    }

    pub fn on(
        &mut self,
        event: crate::entrypoints::sdk::types::HookEvent,
        callback: SdkHookCallback,
    ) {

        if let Ok(mut guard) = self.hooks.try_lock() {
            guard.insert(event, callback);
        }
    }

    fn build_metadata(
        &self,
        duration_ms: u64,
    ) -> crate::entrypoints::sdk::types::SdkMessageMetadata {
        let acc = self.usage.try_lock().ok();
        crate::entrypoints::sdk::types::SdkMessageMetadata {
            model: self.sdk_config.model.clone(),
            input_tokens: acc.as_ref().map(|a| a.input_tokens),
            output_tokens: acc.as_ref().map(|a| a.output_tokens),
            cost_usd: None,
            duration_ms: Some(duration_ms),
        }
    }
}

pub struct SdkEntrypoint {
    config: SdkConfig,

    sessions: Arc<Mutex<HashMap<String, SdkSessionHandle>>>,
}

impl SdkEntrypoint {

    pub fn new(config: SdkConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[allow(clippy::large_futures)]
    pub async fn start_session(&mut self) -> Result<String> {
        let session_id = uuid::Uuid::new_v4().to_string();

        let cwd = self
            .config
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        crate::bootstrap::init_state(cwd.clone());

        let base_config = Config::load_or_init().await?;
        let resolved = self.config.apply_to_config(base_config);

        let svc_data_dir = resolved
            .config_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| cwd.join(".senweavercoding"));
        let _ = crate::services::init_services(crate::services::ServiceContainerConfig {
            data_dir: svc_data_dir,
            team_sync_enabled: resolved.teams.sync_enabled,
            ..Default::default()
        });

        let agent = Agent::from_config(
            &resolved,
            if self.config.denied_tools.is_empty() {
                None
            } else {
                Some(self.config.denied_tools.clone())
            },
            None,
        )
        .await?;

        let usage_acc = Arc::new(Mutex::new(SdkUsageAccumulator::default()));
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let session = SdkSession {
            sdk_config: self.config.clone(),
            agent: Arc::new(Mutex::new(Some(agent))),
            usage: usage_acc,
            hooks: Arc::new(Mutex::new(HashMap::new())),
            stopped,
        };

        let sessions = Arc::clone(&self.sessions);
        sessions
            .lock()
            .await
            .insert(session_id.clone(), Arc::new(Mutex::new(Some(session))));

        tracing::info!(session_id = %session_id, "SDK session started");
        Ok(session_id)
    }

    pub async fn session(&self, session_id: &str) -> Option<SdkSessionHandle> {
        self.sessions.lock().await.get(session_id).cloned()
    }

    #[allow(clippy::let_and_return, clippy::large_futures)]
    pub async fn send_message(&mut self, message: SdkMessage) -> Result<SdkMessage> {
        let session_id = self.start_session().await?;
        let session_arc = self.session(&session_id).await;
        let session_guard = session_arc.ok_or_else(|| anyhow::anyhow!("session not found"))?;
        let mut session_lock = session_guard.lock().await;
        let session = session_lock
            .take()
            .ok_or_else(|| anyhow::anyhow!("session already used (single-use mode)"))?;
        drop(session_lock);
        self.sessions.lock().await.remove(&session_id);

        session.send_message(message).await
    }

    #[allow(clippy::large_futures)]
    pub async fn send_message_streamed(
        &mut self,
        message: &SdkMessage,
        event_tx: mpsc::Sender<SdkTurnEvent>,
    ) -> Result<SdkMessage> {
        let session_id = self.start_session().await?;
        let session_arc = self.session(&session_id).await;
        let session_guard = session_arc.ok_or_else(|| anyhow::anyhow!("session not found"))?;
        let mut session_lock = session_guard.lock().await;
        let session = session_lock
            .take()
            .ok_or_else(|| anyhow::anyhow!("session already used (single-use mode)"))?;
        drop(session_lock);
        self.sessions.lock().await.remove(&session_id);
        session.send_message_streamed(message, event_tx).await
    }

    pub async fn stop_session(&self, session_id: &str) -> Result<()> {
        let sessions = self.sessions.lock().await;
        if let Some(session_arc) = sessions.get(session_id) {
            let mut guard = session_arc.lock().await;
            if let Some(ref mut session) = *guard {
                session.stop().await?;
            }
        }
        Ok(())
    }

    pub async fn stop_all(&self) -> Result<()> {
        let sessions: Vec<_> = self.sessions.lock().await.drain().collect();
        for (_, session_arc) in sessions {
            let mut guard = session_arc.lock().await;
            if let Some(ref mut session) = *guard {
                let _ = session.stop().await;
            }
        }
        Ok(())
    }
}

pub struct SdkToolCallBuilder {
    id: String,
    name: String,
    input: serde_json::Value,
    output: Option<String>,
    is_error: bool,
}

impl SdkToolCallBuilder {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input: serde_json::Value::Null,
            output: None,
            is_error: false,
        }
    }

    pub fn input(mut self, input: serde_json::Value) -> Self {
        self.input = input;
        self
    }

    pub fn output(mut self, output: impl Into<String>) -> Self {
        self.output = Some(output.into());
        self
    }

    pub fn error(mut self) -> Self {
        self.is_error = true;
        self
    }

    pub fn build(self) -> SdkToolCall {
        SdkToolCall {
            id: self.id,
            name: self.name,
            input: self.input,
            output: self.output,
            is_error: self.is_error,
        }
    }
}
