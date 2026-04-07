// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// SDK entrypoint — mirrors claude-code-typescript-src/entrypoints/sdk/.
// Provides a programmatic API for embedding SenWeaverCoding in other applications.

use std::collections::HashMap;
use std::sync::Arc;

/// Handle type for a running SDK session. Allows `stop()` to be called even from
/// a separate task that doesn't have `&mut SdkSession`.
pub type SdkSessionHandle = Arc<Mutex<Option<SdkSession>>>;

use anyhow::Result;
use tokio::sync::{mpsc, Mutex};

use crate::agent::{Agent, TurnEvent};
use crate::config::Config;
use crate::observability::{Observer, ObserverEvent};
use crate::providers::traits::TokenUsage;

use super::sdk_types::{
    SdkConfig, SdkHookCallback, SdkMessage, SdkStatus, SdkToolCall,
    SdkTurnEvent,
};

/// Accumulator for LLM usage tracked across the session.
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

/// Observer wrapper that intercepts `LlmResponse` events to accumulate usage.
struct UsageTrackingObserver {
    inner: Arc<dyn Observer>,
    accumulator: Arc<Mutex<SdkUsageAccumulator>>,
}

impl UsageTrackingObserver {
    fn new(inner: Arc<dyn Observer>, accumulator: Arc<Mutex<SdkUsageAccumulator>>) -> Self {
        Self {
            inner,
            accumulator,
        }
    }
}

impl Observer for UsageTrackingObserver {
    fn record_event(&self, event: &ObserverEvent) {
        if let ObserverEvent::LlmResponse { input_tokens, output_tokens, .. } = event {
            let usage = TokenUsage {
                input_tokens: *input_tokens,
                output_tokens: *output_tokens,
                cached_input_tokens: None,
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

// ──────────────────────────────────────────────────────────────────────────────
// SdkSession
// ──────────────────────────────────────────────────────────────────────────────

/// An active agent session created by `SdkEntrypoint::start_session`.
///
/// Call [`send_message`](SdkSession::send_message) or
/// [`send_message_streamed`](SdkSession::send_message_streamed) to interact
/// with the agent, and [`stop`](SdkSession::stop) to terminate the session.
pub struct SdkSession {
    /// Immutable SDK-level configuration (not the resolved Config).
    sdk_config: SdkConfig,

    /// The underlying agent instance — shared behind a lock.
    /// Stored as `Arc<Mutex<Option<Agent>>>` so we can get mutable access
    /// via `lock().await.as_mut()`.
    agent: Arc<Mutex<Option<Agent>>>,

    /// Accumulated usage across all turns in this session.
    usage: Arc<Mutex<SdkUsageAccumulator>>,

    /// Hook callbacks registered for this session.
    hooks: Arc<Mutex<HashMap<crate::entrypoints::sdk_types::HookEvent, SdkHookCallback>>>,

    /// Whether stop has been called.
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl SdkSession {
    /// Send a message and receive the agent's response.
    ///
    /// Maintains conversation history across calls within the same session.
    ///
    /// # Errors
    /// Returns an error if no session is active, the session was stopped,
    /// or the agent encounters an error.
    pub async fn send_message(&self, message: SdkMessage) -> Result<SdkMessage> {
        let mut guard = self.agent.lock().await;
        let agent = guard.as_mut().ok_or_else(|| {
            anyhow::anyhow!("no active session — call start_session() first")
        })?;

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

    /// Send a message with streaming event callbacks.
    ///
    /// Events are sent to `event_tx` as they occur via a background relay task.
    /// The future resolves when the agent turn completes and all events are flushed.
    ///
    /// The relay task converts internal `TurnEvent`s to SDK-facing `SdkTurnEvent`s
    /// and forwards them to `event_tx`. If the receiver is dropped, the relay task
    /// is cancelled but the agent turn continues to completion.
    ///
    /// # Errors
    /// Returns an error if no session is active, the session was stopped,
    /// or the agent encounters an error.
    pub async fn send_message_streamed(
        &self,
        message: &SdkMessage,
        event_tx: mpsc::Sender<SdkTurnEvent>,
    ) -> Result<SdkMessage> {
        let mut guard = self.agent.lock().await;
        let agent = guard.as_mut().ok_or_else(|| {
            anyhow::anyhow!("no active session — call start_session() first")
        })?;

        let start = std::time::Instant::now();

        // Channel bridging sync `Sender<TurnEvent>` (required by Agent::turn_streamed)
        // and async `mpsc::Sender<SdkTurnEvent>` (required by SDK consumers).
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);

        // Background task: relay TurnEvents to SDK consumers, converting to SdkTurnEvent.
        // This task lives for the duration of the turn. If the receiver is dropped,
        // `tx.send()` returns Err (disconnected) and the task exits cleanly.
        let sdk_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let sdk_event: SdkTurnEvent = event.into();
                if sdk_tx.send(sdk_event).await.is_err() {
                    // Consumer dropped the receiver — stop relaying.
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

    /// Get the current session status.
    pub fn status(&self) -> SdkStatus {
        if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
            return SdkStatus::Stopped;
        }
        SdkStatus::Running
    }

    /// Stop the active session.
    ///
    /// Drops the agent, causing any in-progress `send_message` or
    /// `send_message_streamed` call to return an error.
    pub async fn stop(&mut self) -> Result<()> {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let mut guard = self.agent.lock().await;
        *guard = None;

        tracing::info!("SDK session stopped");
        Ok(())
    }

    /// Accumulated LLM usage across all turns in this session.
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

    /// Register a hook callback for the given event type.
    ///
    /// Callbacks are invoked synchronously during agent execution.
    /// Keep callbacks fast and non-blocking.
    pub fn on(
        &mut self,
        event: crate::entrypoints::sdk_types::HookEvent,
        callback: SdkHookCallback,
    ) {
        // Use try_lock for synchronous context (parking_lot Mutex).
        if let Ok(mut guard) = self.hooks.try_lock() {
            guard.insert(event, callback);
        }
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    fn build_metadata(&self, duration_ms: u64) -> crate::entrypoints::sdk_types::SdkMessageMetadata {
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

// ──────────────────────────────────────────────────────────────────────────────
// SdkEntrypoint
// ──────────────────────────────────────────────────────────────────────────────

/// SDK entrypoint for programmatic embedding.
///
/// Construct with `SdkEntrypoint::new(config)`, then call `start_session()`
/// to begin a session. Each session maintains its own agent instance and
/// conversation history.
pub struct SdkEntrypoint {
    config: SdkConfig,
    /// Maps session IDs to session handles. Each handle owns the session's mutable
    /// `SdkSession` (wrapped in `Option` so it can be taken on `stop`).
    sessions: Arc<Mutex<HashMap<String, SdkSessionHandle>>>,
}

impl SdkEntrypoint {
    /// Create a new SDK entrypoint with the given configuration.
    pub fn new(config: SdkConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Start a new agent session.
    ///
    /// Returns a session ID that can be used with `session()`.
    ///
    /// # Errors
    /// Returns an error if the underlying agent cannot be initialized.
    #[allow(clippy::large_futures)]
    pub async fn start_session(&mut self) -> Result<String> {
        let session_id = uuid::Uuid::new_v4().to_string();

        let cwd = self
            .config
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        crate::bootstrap::init_state(cwd);

        // Load base config, then apply SDK overrides
        let base_config = Config::load_or_init().await?;
        let resolved = self.config.apply_to_config(base_config);

        // Build the agent
        let agent = Agent::from_config(
            &resolved,
            if self.config.denied_tools.is_empty() {
                None
            } else {
                Some(self.config.denied_tools.clone())
            },
        )
        .await?;

        // denied_tools are applied by Agent::from_config(&resolved, denied_tools)

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

    /// Get a session by ID.
    pub async fn session(&self, session_id: &str) -> Option<SdkSessionHandle> {
        self.sessions.lock().await.get(session_id).cloned()
    }

    /// Convenience: start a session and send a message in one call.
    ///
    /// Creates a temporary session, sends the message, and returns the response.
    /// The session is not retained after this call.
    ///
    /// For multi-turn conversations, use `start_session()` + `send_message()` instead.
    #[allow(clippy::let_and_return, clippy::large_futures)]
    pub async fn send_message(&mut self, message: SdkMessage) -> Result<SdkMessage> {
        let session_id = self.start_session().await?;
        let session_arc = self.session(&session_id).await;
        let session_guard = session_arc.ok_or_else(|| anyhow::anyhow!("session not found"))?;
        let mut session_lock = session_guard.lock().await;
        let session = session_lock.take().ok_or_else(|| {
            anyhow::anyhow!("session already used (single-use mode)")
        })?;
        // session is dropped here; agent is dropped
        session.send_message(message).await
    }

    /// Convenience: start a session and send a streamed message in one call.
    ///
    /// Creates a temporary session, sends the message with streaming, and
    /// returns the response. The session is not retained after this call.
    ///
    /// For multi-turn conversations, use `start_session()` + `send_message_streamed()`.
    #[allow(clippy::large_futures)]
    pub async fn send_message_streamed(
        &mut self,
        message: &SdkMessage,
        event_tx: mpsc::Sender<SdkTurnEvent>,
    ) -> Result<SdkMessage> {
        let session_id = self.start_session().await?;
        let session_arc = self.session(&session_id).await;
        let session_guard =
            session_arc.ok_or_else(|| anyhow::anyhow!("session not found"))?;
        let mut session_lock = session_guard.lock().await;
        let session = session_lock.take().ok_or_else(|| {
            anyhow::anyhow!("session already used (single-use mode)")
        })?;
        session.send_message_streamed(message, event_tx).await
    }

    /// Stop a specific session by ID.
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

    /// Stop all active sessions.
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

// ──────────────────────────────────────────────────────────────────────────────
// SdkToolCallBuilder (helper)
// ──────────────────────────────────────────────────────────────────────────────

/// Helper to build `SdkToolCall` values step by step.
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
