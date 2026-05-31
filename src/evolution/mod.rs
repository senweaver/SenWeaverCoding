// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod cloud;
pub mod collector;
pub mod distiller;
pub mod evaluators;
pub mod exporter;
pub mod injector;
pub mod judge;
pub mod recycling;
pub mod reflection;
pub mod reward;
pub mod store;
pub mod types;

use anyhow::Result;
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub use collector::{
    EVOLUTION_CTX, EvolutionCtx, finalize_turn, observe_tool_outcome, record_cost,
    record_provider_model, record_tool_outcome, scope_evolution_ctx, set_response_text,
    set_thinking_text, try_ctx,
};

pub use distiller::DistillRequest;

pub use evaluators::{FastEvaluator, run_fast_evaluators, user_thumbs::score_from_vote};

pub use cloud::push_export_to_target;

pub use exporter::{ExportFilter, ExportOptions, ExportPreview, export_to_file, preview_export};

pub use injector::build_lesson_block;

pub use judge::{JudgeProviderRef, JudgeRequest, parse_boxed_verdict};

pub use recycling::{
    RecycledExperience, RecycledExperienceOutcome, RecyclingHarvestReport, RecyclingStore,
    build_recycled_block,
};

pub use reflection::{
    REFLECTION_QUEUE_CAPACITY, ReflectionLesson, ReflectionLessonKind, ReflectionRequest,
    ReflectionRun, ReflectionRunStatus, ReflectionStore, ReflectionSummary,
    ReflectionTriggerCause, ReflectionWritebackReport,
};

pub use reward::{fuse_signals, merge_signal};

pub use store::Store;

pub use types::{
    AnthropicBlockView, AnthropicMessageView, AuditEvent, ChatMessageView, CloudTarget,
    CloudTargetKind, CostView, EvolutionConfig, EvolutionExportConfig, EvolutionExportFormat,
    EvolutionSignalWeights, ExperienceRecyclingConfig, ExportRecord, Lesson, NextStateView,
    PersistenceStatus, Playbook, PurgeReport, PurgeScope, PushReceipt, ReflectionDepth,
    ReflectionTriggerMode, ReflectionWritebackTarget, ResponseView, Reward,
    SelfReflectionConfig, SignalScore, SignalSource, ThumbVote, ToolCallView, ToolOutcome,
    TurnClass, TurnRecord,
};

#[derive(Debug, Clone)]
pub struct RegisteredModel {
    pub provider_id: String,
    pub model: String,
}

#[derive(Clone)]
pub struct ResolvedReflectionProvider {
    pub provider: Arc<dyn crate::providers::Provider>,
    pub model: String,
    pub provider_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ThumbVoteSubmission {
    pub vote: ThumbVote,
    pub coding_mode: Option<String>,
}

struct RecentTurnEntry {
    turn_id: String,
    response: String,
    coding_mode: Option<String>,
    completed_at: Instant,
}

struct SessionSignalEntry {
    failures: u32,
    last_failure_at: Instant,
    thumbs_down: u32,
    last_thumbs_down_at: Instant,
}

pub struct EvolutionEngine {
    store: Arc<Store>,
    config: RwLock<EvolutionConfig>,
    workspace_dir: PathBuf,
    judge_tx: RwLock<Option<mpsc::Sender<JudgeRequest>>>,
    distill_tx: RwLock<Option<mpsc::Sender<DistillRequest>>>,
    reflection_tx: RwLock<Option<mpsc::Sender<ReflectionRequest>>>,
    judge_provider: RwLock<Option<JudgeProviderRef>>,
    reflection_providers: RwLock<HashMap<String, JudgeProviderRef>>,
    registered_models: RwLock<Vec<RegisteredModel>>,
    worker_started: AtomicBool,
    scheduler_started: AtomicBool,
    recent_turns: Mutex<HashMap<String, RecentTurnEntry>>,
    recycling_store: RwLock<Option<Arc<RecyclingStore>>>,
    reflection_store: RwLock<Option<Arc<ReflectionStore>>>,
    reflection_store_bind_error: RwLock<Option<String>>,
    recycling_store_bind_error: RwLock<Option<String>>,
    session_signals: Mutex<HashMap<String, SessionSignalEntry>>,
    judge_enqueued_total: AtomicU64,
    judge_processed_total: AtomicU64,
    judge_last_error_at: RwLock<Option<DateTime<Utc>>>,
    judge_last_error_message: RwLock<Option<String>>,
    judge_worker_running: AtomicBool,
    reflection_scheduler_running: AtomicBool,
    reflection_scheduler_last_tick_at: RwLock<Option<DateTime<Utc>>>,
    recycling_total_harvested: AtomicU64,
    recycling_last_harvest_at: RwLock<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone)]
pub struct JudgeWorkerMetrics {
    pub running: bool,
    pub enqueued_total: u64,
    pub processed_total: u64,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReflectionSchedulerMetrics {
    pub running: bool,
    pub interval_minutes: u32,
    pub last_tick_at: Option<DateTime<Utc>>,
    pub next_tick_at_estimate: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct RecyclingMetrics {
    pub total_harvested: u64,
    pub recent_24h_harvested: u64,
    pub last_harvest_at: Option<DateTime<Utc>>,
}

impl EvolutionEngine {
    pub fn new(workspace_dir: PathBuf, config: EvolutionConfig) -> Result<Arc<Self>> {
        let base_dir = resolve_base_dir(&workspace_dir, &config);
        let store = Arc::new(Store::open(base_dir, config.persist_training_data)?);
        let shared_db = store.shared_connection();
        let mut recycling_bind_error: Option<String> = None;
        let recycling_store = match RecyclingStore::bind(Arc::clone(&shared_db)) {
            Ok(s) => Some(Arc::new(s)),
            Err(error) => {
                let msg = error.to_string();
                tracing::warn!(error = %msg, "evolution: failed to bind recycling store");
                recycling_bind_error = Some(msg);
                None
            }
        };
        let mut reflection_bind_error: Option<String> = None;
        let reflection_store = match ReflectionStore::bind(shared_db) {
            Ok(s) => Some(Arc::new(s)),
            Err(error) => {
                let msg = error.to_string();
                tracing::warn!(error = %msg, "evolution: failed to bind reflection store");
                reflection_bind_error = Some(msg);
                None
            }
        };
        Ok(Arc::new(Self {
            store,
            config: RwLock::new(config),
            workspace_dir,
            judge_tx: RwLock::new(None),
            distill_tx: RwLock::new(None),
            reflection_tx: RwLock::new(None),
            judge_provider: RwLock::new(None),
            reflection_providers: RwLock::new(HashMap::new()),
            registered_models: RwLock::new(Vec::new()),
            worker_started: AtomicBool::new(false),
            scheduler_started: AtomicBool::new(false),
            recent_turns: Mutex::new(HashMap::new()),
            recycling_store: RwLock::new(recycling_store),
            reflection_store: RwLock::new(reflection_store),
            reflection_store_bind_error: RwLock::new(reflection_bind_error),
            recycling_store_bind_error: RwLock::new(recycling_bind_error),
            session_signals: Mutex::new(HashMap::new()),
            judge_enqueued_total: AtomicU64::new(0),
            judge_processed_total: AtomicU64::new(0),
            judge_last_error_at: RwLock::new(None),
            judge_last_error_message: RwLock::new(None),
            judge_worker_running: AtomicBool::new(false),
            reflection_scheduler_running: AtomicBool::new(false),
            reflection_scheduler_last_tick_at: RwLock::new(None),
            recycling_total_harvested: AtomicU64::new(0),
            recycling_last_harvest_at: RwLock::new(None),
        }))
    }

    pub fn recycling_store(&self) -> Option<Arc<RecyclingStore>> {
        self.recycling_store.read().as_ref().map(Arc::clone)
    }

    pub fn reflection_store(&self) -> Option<Arc<ReflectionStore>> {
        self.reflection_store.read().as_ref().map(Arc::clone)
    }

    pub fn record_recent_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        response: &str,
        coding_mode: Option<&str>,
    ) {
        let mut guard = self.recent_turns.lock();
        let entry = RecentTurnEntry {
            turn_id: turn_id.to_string(),
            response: response.to_string(),
            coding_mode: coding_mode.map(str::to_string),
            completed_at: Instant::now(),
        };
        guard.insert(session_id.to_string(), entry);
        let cutoff = Duration::from_secs(60 * 60);
        guard.retain(|_, e| e.completed_at.elapsed() < cutoff);
    }

    pub fn flush_next_state(&self, session_id: &str, role: &str, content: &str) {
        let entry = {
            let mut guard = self.recent_turns.lock();
            guard.remove(session_id)
        };
        let Some(entry) = entry else {
            return;
        };
        let snapshot = self.config_snapshot();
        if !snapshot.next_state_judge_enabled {
            return;
        }
        let request = JudgeRequest {
            turn_id: entry.turn_id,
            session_id: session_id.to_string(),
            prev_response: entry.response,
            next_state: NextStateView {
                role: role.to_string(),
                content: content.to_string(),
            },
            coding_mode: entry.coding_mode,
        };
        let _ = self.enqueue_judge(request);
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    pub fn config_snapshot(&self) -> EvolutionConfig {
        self.config.read().clone()
    }

    pub fn set_config(&self, mut config: EvolutionConfig) {
        if (config.reflection.enabled || config.recycling.enabled) && !config.persist_training_data {
            config.persist_training_data = true;
        }
        self.store
            .set_persist_training_data(config.persist_training_data);
        *self.config.write() = config;
    }

    pub fn reflection_store_health(&self) -> Option<String> {
        if self.reflection_store.read().is_some() {
            return None;
        }
        self.reflection_store_bind_error
            .read()
            .clone()
            .or_else(|| Some("reflection_store_bind_failed".to_string()))
    }

    pub fn recycling_store_health(&self) -> Option<String> {
        if self.recycling_store.read().is_some() {
            return None;
        }
        self.recycling_store_bind_error
            .read()
            .clone()
            .or_else(|| Some("recycling_store_bind_failed".to_string()))
    }

    pub fn set_persist_training_data(&self, value: bool) {
        self.store.set_persist_training_data(value);
        self.config.write().persist_training_data = value;
    }

    pub fn enabled(&self) -> bool {
        self.config.read().enabled
    }

    pub fn persist_training_data(&self) -> bool {
        self.store.persist_training_data()
    }

    pub fn set_judge_provider(&self, reference: JudgeProviderRef) {
        *self.judge_provider.write() = Some(reference);
    }

    pub fn judge_provider(&self) -> Option<JudgeProviderRef> {
        self.judge_provider.read().clone()
    }

    pub fn register_reflection_provider(&self, provider_id: &str, reference: JudgeProviderRef) {
        let mut guard = self.reflection_providers.write();
        guard.insert(provider_id.to_string(), reference);
    }

    pub fn clear_reflection_providers(&self) {
        self.reflection_providers.write().clear();
    }

    pub fn reflection_provider_for(&self, provider_id: &str) -> Option<JudgeProviderRef> {
        self.reflection_providers.read().get(provider_id).cloned()
    }

    pub fn set_registered_models(&self, models: Vec<RegisteredModel>) {
        *self.registered_models.write() = models;
    }

    pub fn registered_models(&self) -> Vec<RegisteredModel> {
        self.registered_models.read().clone()
    }

    pub fn has_registered_models(&self) -> bool {
        !self.registered_models.read().is_empty()
    }

    pub fn is_model_registered(&self, model: &str) -> bool {
        let needle = model.trim();
        if needle.is_empty() {
            return false;
        }
        let guard = self.registered_models.read();
        if guard.is_empty() {
            return true;
        }
        guard.iter().any(|m| m.model == needle)
    }

    pub fn resolve_reflection_provider(&self) -> Option<ResolvedReflectionProvider> {
        let snapshot = self.config_snapshot();
        let reflection_cfg = snapshot.reflection.clone();
        if let Some(provider_id) = reflection_cfg
            .reflection_provider
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(reference) = self.reflection_provider_for(provider_id) {
                let model = reflection_cfg
                    .reflection_model
                    .clone()
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or(reference.model.clone());
                return Some(ResolvedReflectionProvider {
                    provider: reference.provider,
                    model,
                    provider_id: Some(provider_id.to_string()),
                });
            }
        }
        let judge = self.judge_provider()?;
        let model = reflection_cfg
            .reflection_model
            .clone()
            .or(snapshot.judge_model.clone())
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(judge.model.clone());
        Some(ResolvedReflectionProvider {
            provider: judge.provider,
            model,
            provider_id: None,
        })
    }

    pub fn submit_thumb_vote(
        self: &Arc<Self>,
        vote: ThumbVote,
        coding_mode: Option<&str>,
    ) -> Result<types::Reward> {
        self.store.record_thumb(&vote)?;
        let signal = evaluators::user_thumbs::score_from_vote(&vote);
        let weights = self.config_snapshot().signal_weights;
        let merged = self
            .store
            .merge_turn_signal(&vote.turn_id, &signal, &weights)?;
        if vote.score < 0 {
            self.record_thumbs_down(&vote.session_id, coding_mode);
        }
        Ok(merged)
    }

    pub fn ensure_judge_worker(self: &Arc<Self>) {
        if self
            .worker_started
            .swap(true, Ordering::SeqCst)
        {
            return;
        }
        let (judge_tx, judge_rx) = mpsc::channel::<JudgeRequest>(judge::JUDGE_QUEUE_CAPACITY);
        *self.judge_tx.write() = Some(judge_tx);
        let engine_judge = Arc::clone(self);
        crate::runtime::spawn_supervised("evolution.judge_worker", async move {
            engine_judge.mark_judge_worker_running(true);
            judge::run_judge_worker(Arc::clone(&engine_judge), judge_rx).await;
            engine_judge.mark_judge_worker_running(false);
        });
        let (distill_tx, distill_rx) =
            mpsc::channel::<DistillRequest>(distiller::DISTILL_QUEUE_CAPACITY);
        *self.distill_tx.write() = Some(distill_tx);
        let engine_distill = Arc::clone(self);
        crate::runtime::spawn_supervised("evolution.distill_worker", async move {
            distiller::run_distill_worker(engine_distill, distill_rx).await;
        });
        let (reflection_tx, reflection_rx) =
            mpsc::channel::<ReflectionRequest>(reflection::REFLECTION_QUEUE_CAPACITY);
        *self.reflection_tx.write() = Some(reflection_tx);
        let engine_reflection = Arc::clone(self);
        crate::runtime::spawn_supervised("evolution.reflection_worker", async move {
            reflection::run_reflection_worker(engine_reflection, reflection_rx).await;
        });
    }

    pub fn ensure_reflection_scheduler(self: &Arc<Self>) {
        if self
            .scheduler_started
            .swap(true, Ordering::SeqCst)
        {
            return;
        }
        let engine = Arc::clone(self);
        crate::runtime::spawn_supervised("evolution.reflection_scheduler", async move {
            engine.mark_reflection_scheduler_running(true);
            run_reflection_scheduler(Arc::clone(&engine)).await;
            engine.mark_reflection_scheduler_running(false);
        });
    }

    pub fn enqueue_reflection(self: &Arc<Self>, request: ReflectionRequest) -> Result<()> {
        let snapshot = self.config_snapshot();
        if !snapshot.reflection.enabled {
            return Ok(());
        }
        if let Some(tx) = self.reflection_tx.read().clone() {
            if let Err(error) = tx.try_send(request) {
                tracing::debug!(error = %error, "evolution reflection queue full or closed");
            }
        }
        Ok(())
    }

    pub fn enqueue_reflection_strict(
        self: &Arc<Self>,
        request: ReflectionRequest,
    ) -> Result<()> {
        let snapshot = self.config_snapshot();
        if !snapshot.reflection.enabled {
            anyhow::bail!("reflection_disabled");
        }
        let tx = match self.reflection_tx.read().clone() {
            Some(tx) => tx,
            None => anyhow::bail!("reflection_worker_unavailable"),
        };
        match tx.try_send(request) {
            Ok(()) => Ok(()),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                anyhow::bail!("reflection_queue_full")
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                anyhow::bail!("reflection_worker_unavailable")
            }
        }
    }

    pub fn record_turn_signal(self: &Arc<Self>, turn: &TurnRecord) {
        let snapshot = self.config_snapshot();
        if !snapshot.reflection.enabled {
            return;
        }
        let session_id = turn.session_id.clone();
        let mut should_trigger = false;
        {
            let mut guard = self.session_signals.lock();
            let entry = guard.entry(session_id.clone()).or_insert_with(|| SessionSignalEntry {
                failures: 0,
                last_failure_at: Instant::now(),
                thumbs_down: 0,
                last_thumbs_down_at: Instant::now(),
            });
            if turn.reward.final_score < 0.0 || turn.aborted.is_some() {
                entry.failures = entry.failures.saturating_add(1);
                entry.last_failure_at = Instant::now();
                if matches!(
                    snapshot.reflection.trigger_mode,
                    ReflectionTriggerMode::Auto
                ) && entry.failures >= snapshot.reflection.failure_threshold
                {
                    should_trigger = true;
                    entry.failures = 0;
                }
            }
            let cutoff = Duration::from_secs(60 * 60 * 6);
            guard.retain(|_, e| {
                e.last_failure_at.elapsed() < cutoff
                    || e.last_thumbs_down_at.elapsed() < cutoff
            });
        }
        if should_trigger {
            self.schedule_session_reflection(&session_id, ReflectionTriggerCause::FailureThreshold);
        }
    }

    pub fn record_thumbs_down(self: &Arc<Self>, session_id: &str, coding_mode: Option<&str>) {
        let snapshot = self.config_snapshot();
        if !snapshot.reflection.enabled {
            return;
        }
        if !snapshot.reflection.include_user_thumbs_down {
            return;
        }
        let mut should_trigger = false;
        {
            let mut guard = self.session_signals.lock();
            let entry = guard
                .entry(session_id.to_string())
                .or_insert_with(|| SessionSignalEntry {
                    failures: 0,
                    last_failure_at: Instant::now(),
                    thumbs_down: 0,
                    last_thumbs_down_at: Instant::now(),
                });
            entry.thumbs_down = entry.thumbs_down.saturating_add(1);
            entry.last_thumbs_down_at = Instant::now();
            if matches!(
                snapshot.reflection.trigger_mode,
                ReflectionTriggerMode::Auto
            ) {
                should_trigger = true;
                entry.thumbs_down = 0;
            }
        }
        let _ = coding_mode;
        if should_trigger {
            self.schedule_session_reflection(session_id, ReflectionTriggerCause::UserThumbsDown);
        }
    }

    pub fn schedule_session_reflection(
        self: &Arc<Self>,
        session_id: &str,
        cause: ReflectionTriggerCause,
    ) {
        let snapshot = self.config_snapshot();
        if !snapshot.reflection.enabled {
            return;
        }
        let lookback = snapshot.reflection.lookback_turns.max(1);
        let mut turns = self
            .store
            .find_turns_for_session(session_id, lookback)
            .unwrap_or_default();
        if turns.is_empty() {
            return;
        }
        if matches!(cause, ReflectionTriggerCause::SessionEnd)
            && turns.len() < snapshot.reflection.min_turns_for_auto.max(1)
            && matches!(
                snapshot.reflection.trigger_mode,
                ReflectionTriggerMode::Auto
            )
        {
            return;
        }
        let coding_mode = turns
            .iter()
            .find_map(|t| t.coding_mode.clone());
        turns.reverse();
        let request = ReflectionRequest {
            run_id: format!("ref_{}", uuid::Uuid::new_v4().simple()),
            trigger: cause,
            session_id: Some(session_id.to_string()),
            turns,
            coding_mode,
        };
        let _ = self.enqueue_reflection(request);
    }

    pub fn trigger_manual_reflection(
        self: &Arc<Self>,
        session_id: Option<&str>,
    ) -> Result<String> {
        let snapshot = self.config_snapshot();
        if !snapshot.reflection.enabled {
            anyhow::bail!("reflection_disabled");
        }
        if !self.persist_training_data() {
            anyhow::bail!("persistence_required");
        }
        let lookback = snapshot.reflection.lookback_turns.max(1);
        let mut turns = match session_id {
            Some(sid) => self
                .store
                .find_turns_for_session(sid, lookback)
                .unwrap_or_default(),
            None => self.store.find_recent_turns(lookback).unwrap_or_default(),
        };
        if turns.is_empty() {
            anyhow::bail!("no_turns_available");
        }
        let coding_mode = turns
            .iter()
            .find_map(|t| t.coding_mode.clone());
        turns.reverse();
        let run_id = format!("ref_{}", uuid::Uuid::new_v4().simple());
        let request = ReflectionRequest {
            run_id: run_id.clone(),
            trigger: ReflectionTriggerCause::Manual,
            session_id: session_id.map(str::to_string),
            turns,
            coding_mode,
        };
        self.enqueue_reflection_strict(request)?;
        Ok(run_id)
    }

    pub fn enqueue_judge(&self, request: JudgeRequest) -> Result<()> {
        let snapshot = self.config_snapshot();
        if !snapshot.next_state_judge_enabled {
            return Ok(());
        }
        if let Some(tx) = self.judge_tx.read().clone() {
            match tx.try_send(request) {
                Ok(()) => {
                    self.note_judge_enqueued();
                }
                Err(error) => {
                    tracing::debug!(error = %error, "evolution judge queue full or closed");
                }
            }
        }
        Ok(())
    }

    pub fn note_judge_enqueued(&self) {
        self.judge_enqueued_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_judge_processed(&self) {
        self.judge_processed_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_judge_error(&self, message: &str) {
        *self.judge_last_error_at.write() = Some(Utc::now());
        let trimmed: String = message.chars().take(240).collect();
        *self.judge_last_error_message.write() = Some(trimmed);
    }

    pub fn mark_judge_worker_running(&self, running: bool) {
        self.judge_worker_running.store(running, Ordering::Relaxed);
    }

    pub fn mark_reflection_scheduler_running(&self, running: bool) {
        self.reflection_scheduler_running
            .store(running, Ordering::Relaxed);
    }

    pub fn note_reflection_scheduler_tick(&self) {
        *self.reflection_scheduler_last_tick_at.write() = Some(Utc::now());
    }

    pub fn note_recycling_harvested(&self) {
        self.recycling_total_harvested.fetch_add(1, Ordering::Relaxed);
        *self.recycling_last_harvest_at.write() = Some(Utc::now());
    }

    pub fn judge_worker_metrics(&self) -> JudgeWorkerMetrics {
        JudgeWorkerMetrics {
            running: self.judge_worker_running.load(Ordering::Relaxed),
            enqueued_total: self.judge_enqueued_total.load(Ordering::Relaxed),
            processed_total: self.judge_processed_total.load(Ordering::Relaxed),
            last_error_at: *self.judge_last_error_at.read(),
            last_error_message: self.judge_last_error_message.read().clone(),
        }
    }

    pub fn reflection_scheduler_metrics(&self) -> ReflectionSchedulerMetrics {
        let snapshot = self.config_snapshot();
        let interval = snapshot.reflection.schedule_interval_minutes.max(5);
        let last_tick = *self.reflection_scheduler_last_tick_at.read();
        let next_tick = last_tick
            .map(|ts| ts + chrono::Duration::minutes(i64::from(interval)));
        ReflectionSchedulerMetrics {
            running: self.reflection_scheduler_running.load(Ordering::Relaxed),
            interval_minutes: interval,
            last_tick_at: last_tick,
            next_tick_at_estimate: next_tick,
        }
    }

    pub fn recycling_metrics(&self) -> RecyclingMetrics {
        let total_persisted = self
            .recycling_store()
            .and_then(|s| s.total_count().ok())
            .unwrap_or(0);
        let recent_24h = self
            .recycling_store()
            .map(|store| {
                let cutoff = Utc::now() - chrono::Duration::hours(24);
                store
                    .count_since(cutoff.timestamp_millis())
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let last_harvest_at = self
            .recycling_store()
            .and_then(|s| s.last_harvest_at())
            .or(*self.recycling_last_harvest_at.read());
        let total_in_memory = self.recycling_total_harvested.load(Ordering::Relaxed);
        let total_harvested = total_persisted.max(total_in_memory);
        RecyclingMetrics {
            total_harvested,
            recent_24h_harvested: recent_24h,
            last_harvest_at,
        }
    }

    pub fn enqueue_distill(&self, request: DistillRequest) -> Result<()> {
        self.dispatch_distill(request, false)
    }

    pub fn enqueue_distill_forced(&self, request: DistillRequest) -> Result<()> {
        self.dispatch_distill(request, true)
    }

    fn dispatch_distill(&self, request: DistillRequest, force: bool) -> Result<()> {
        if !force {
            let snapshot = self.config_snapshot();
            if !snapshot.auto_distill_on_session_end {
                return Ok(());
            }
        }
        if let Some(tx) = self.distill_tx.read().clone() {
            if let Err(error) = tx.try_send(request) {
                tracing::debug!(error = %error, "evolution distill queue full or closed");
            }
        }
        Ok(())
    }
}

fn resolve_base_dir(workspace_dir: &Path, _cfg: &EvolutionConfig) -> PathBuf {
    workspace_dir.join("state").join("evolution")
}

async fn run_reflection_scheduler(engine: Arc<EvolutionEngine>) {
    loop {
        let snapshot = engine.config_snapshot();
        let cfg = snapshot.reflection.clone();
        if !cfg.enabled
            || !matches!(cfg.trigger_mode, ReflectionTriggerMode::Scheduled)
        {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let wait_secs = u64::from(cfg.schedule_interval_minutes.max(5))
            .saturating_mul(60)
            .max(60);
        tokio::time::sleep(Duration::from_secs(wait_secs)).await;
        engine.note_reflection_scheduler_tick();
        let lookback = cfg.lookback_turns.max(1);
        let mut turns = engine.store.find_recent_turns(lookback).unwrap_or_default();
        if turns.is_empty() {
            continue;
        }
        let coding_mode = turns.iter().find_map(|t| t.coding_mode.clone());
        let session_id = turns.first().map(|t| t.session_id.clone());
        turns.reverse();
        let request = ReflectionRequest {
            run_id: format!("ref_{}", uuid::Uuid::new_v4().simple()),
            trigger: ReflectionTriggerCause::Scheduled,
            session_id,
            turns,
            coding_mode,
        };
        let _ = engine.enqueue_reflection(request);
    }
}

static GLOBAL_ENGINE: OnceLock<Arc<EvolutionEngine>> = OnceLock::new();

pub fn init_global(workspace_dir: PathBuf, config: EvolutionConfig) -> Result<Arc<EvolutionEngine>> {
    if let Some(existing) = GLOBAL_ENGINE.get() {
        existing.set_config(config);
        return Ok(Arc::clone(existing));
    }
    let engine = EvolutionEngine::new(workspace_dir, config)?;
    let _ = GLOBAL_ENGINE.set(Arc::clone(&engine));
    Ok(engine)
}

pub fn try_global() -> Option<Arc<EvolutionEngine>> {
    GLOBAL_ENGINE.get().map(Arc::clone)
}

pub fn global() -> Option<Arc<EvolutionEngine>> {
    try_global()
}
