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
use crate::observability::{Observer, ObserverEvent};
use crate::providers::traits::TokenUsage;

use super::sdk_types::{
    SdkConfig, SdkHookCallback, SdkMessage, SdkStatus, SdkToolCall, SdkTurnEvent,
};

#[derive(Default, Clone)]
struct SdkUsageAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    request_count: u64,
}

impl SdkUsageAccumulator {
    fn update(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens.unwrap_or(0);
        self.output_tokens += usage.output_tokens.unwrap_or(0);
        self.cache_read_tokens += usage.cached_input_tokens.unwrap_or(0);
        self.request_count += 1;
    }
}

struct UsageTrackingObserver {
    inner: Arc<dyn Observer>,
    accumulator: Arc<Mutex<SdkUsageAccumulator>>,
}

impl UsageTrackingObserver {
    fn new(inner: Arc<dyn Observer>, accumulator: Arc<Mutex<SdkUsageAccumulator>>) -> Self {
        Self { inner, accumulator }
    }
}

impl Observer for UsageTrackingObserver {
    fn record_event(&self, event: &ObserverEvent) {
        if let ObserverEvent::LlmResponse {
            input_tokens,
            output_tokens,
            ..
        } = event
        {
            let usage = TokenUsage {
                input_tokens: *input_tokens,
                output_tokens: *output_tokens,
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
            };
            if let Ok(mut acc) = self.accumulator.try_lock() {
                acc.update(&usage);
            }
        }
        self.inner.record_event(event);
    }

    fn record_metric(&self, metric: &crate::observability::traits::ObserverMetric) {
        self.inner.record_metric(metric);
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct SdkSession {

    sdk_config: SdkConfig,

    agent: Arc<Mutex<Option<Agent>>>,

    usage: Arc<Mutex<SdkUsageAccumulator>>,

    hooks: Arc<Mutex<HashMap<crate::entrypoints::sdk_types::HookEvent, SdkHookCallback>>>,

    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl SdkSession {

    pub async fn send_message(&self, message: SdkMessage) -> Result<SdkMessage> {
        let mut guard = self.agent.lock().await;
        let agent = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no active session — call start_session() first"))?;

        let start = std::time::Instant::now();
        let response_text = agent.turn(&message.content).await?;
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
            .ok_or_else(|| anyhow::anyhow!("no active session — call start_session() first"))?;

        let start = std::time::Instant::now();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);

        let sdk_tx = event_tx.clone();
        let _relay_task =
            crate::runtime::spawn_supervised("entrypoints.sdk.event_relay", async move {
                while let Some(event) = rx.recv().await {
                    let sdk_event: SdkTurnEvent = event.into();
                    if sdk_tx.send(sdk_event).await.is_err() {

                        break;
                    }
                }
            });

        let response_text = agent.turn_streamed(&message.content, tx).await?;
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let metadata = self.build_metadata(duration_ms);

        Ok(SdkMessage {
            role: "assistant".to_string(),
            content: response_text,
            tool_calls: Vec::new(),
            metadata: Some(metadata),
        })
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

    pub fn session_usage(&self) -> crate::entrypoints::sdk_types::SdkModelUsage {
        let acc = self
            .usage
            .try_lock()
            .map(|guard| (*guard).clone())
            .unwrap_or_default();
        crate::entrypoints::sdk_types::SdkModelUsage {
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
        event: crate::entrypoints::sdk_types::HookEvent,
        callback: SdkHookCallback,
    ) {

        if let Ok(mut guard) = self.hooks.try_lock() {
            guard.insert(event, callback);
        }
    }

    fn build_metadata(
        &self,
        duration_ms: u64,
    ) -> crate::entrypoints::sdk_types::SdkMessageMetadata {
        let acc = self.usage.try_lock().ok();
        crate::entrypoints::sdk_types::SdkMessageMetadata {
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

        crate::bootstrap::init_state(cwd);

        let base_config = Config::load_or_init().await?;
        let resolved = self.config.apply_to_config(base_config);

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
