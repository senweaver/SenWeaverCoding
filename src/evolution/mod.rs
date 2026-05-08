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
pub mod reward;
pub mod store;
pub mod types;

use anyhow::Result;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

pub use reward::{fuse_signals, merge_signal};

pub use store::Store;

pub use types::{
    AnthropicBlockView, AnthropicMessageView, AuditEvent, ChatMessageView, CloudTarget,
    CloudTargetKind, CostView, EvolutionConfig, EvolutionExportConfig, EvolutionExportFormat,
    EvolutionSignalWeights, ExportRecord, Lesson, NextStateView, PersistenceStatus, Playbook,
    PurgeReport, PurgeScope, PushReceipt, ResponseView, Reward, SignalScore, SignalSource,
    ThumbVote, ToolCallView, ToolOutcome, TurnClass, TurnRecord,
};

struct RecentTurnEntry {
    turn_id: String,
    response: String,
    coding_mode: Option<String>,
    completed_at: Instant,
}

pub struct EvolutionEngine {
    store: Arc<Store>,
    config: RwLock<EvolutionConfig>,
    workspace_dir: PathBuf,
    judge_tx: RwLock<Option<mpsc::Sender<JudgeRequest>>>,
    distill_tx: RwLock<Option<mpsc::Sender<DistillRequest>>>,
    judge_provider: RwLock<Option<JudgeProviderRef>>,
    worker_started: std::sync::atomic::AtomicBool,
    recent_turns: Mutex<HashMap<String, RecentTurnEntry>>,
}

impl EvolutionEngine {
    pub fn new(workspace_dir: PathBuf, config: EvolutionConfig) -> Result<Arc<Self>> {
        let base_dir = resolve_base_dir(&workspace_dir, &config);
        let store = Arc::new(Store::open(base_dir, config.persist_training_data)?);
        Ok(Arc::new(Self {
            store,
            config: RwLock::new(config),
            workspace_dir,
            judge_tx: RwLock::new(None),
            distill_tx: RwLock::new(None),
            judge_provider: RwLock::new(None),
            worker_started: std::sync::atomic::AtomicBool::new(false),
            recent_turns: Mutex::new(HashMap::new()),
        }))
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

    pub fn set_config(&self, config: EvolutionConfig) {
        self.store
            .set_persist_training_data(config.persist_training_data);
        *self.config.write() = config;
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

    pub fn ensure_judge_worker(self: &Arc<Self>) {
        if self
            .worker_started
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let (judge_tx, judge_rx) = mpsc::channel::<JudgeRequest>(judge::JUDGE_QUEUE_CAPACITY);
        *self.judge_tx.write() = Some(judge_tx);
        let engine_judge = Arc::clone(self);
        tokio::spawn(async move {
            judge::run_judge_worker(engine_judge, judge_rx).await;
        });
        let (distill_tx, distill_rx) =
            mpsc::channel::<DistillRequest>(distiller::DISTILL_QUEUE_CAPACITY);
        *self.distill_tx.write() = Some(distill_tx);
        let engine_distill = Arc::clone(self);
        tokio::spawn(async move {
            distiller::run_distill_worker(engine_distill, distill_rx).await;
        });
    }

    pub fn enqueue_judge(&self, request: JudgeRequest) -> Result<()> {
        let snapshot = self.config_snapshot();
        if !snapshot.next_state_judge_enabled {
            return Ok(());
        }
        if let Some(tx) = self.judge_tx.read().clone() {
            if let Err(error) = tx.try_send(request) {
                tracing::debug!(error = %error, "evolution judge queue full or closed");
            }
        }
        Ok(())
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
