// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Process-wide bootstrap state, analogous to claude-code-typescript-src`bootstrap/state.ts`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Session ID
// ---------------------------------------------------------------------------

/// Opaque session identifier (UUID v4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Model usage tracking (per-model)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub request_count: u64,
    pub total_cost_usd: f64,
}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// Ephemeral state for the current session, matching the fields from
/// claude-code's `State` type in `bootstrap/state.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    // -- identity --
    pub session_id: SessionId,
    pub parent_session_id: Option<SessionId>,
    pub original_cwd: PathBuf,
    pub project_root: PathBuf,
    pub cwd: PathBuf,

    // -- cost / usage --
    pub total_cost_usd: f64,
    pub total_api_duration_ms: u64,
    pub total_api_duration_without_retries_ms: u64,
    pub total_tool_duration_ms: u64,
    pub total_lines_added: u64,
    pub total_lines_removed: u64,
    pub has_unknown_model_cost: bool,
    pub model_usage: HashMap<String, ModelUsage>,

    // -- turn-level counters (reset each turn) --
    pub turn_hook_duration_ms: u64,
    pub turn_tool_duration_ms: u64,
    pub turn_classifier_duration_ms: u64,
    pub turn_tool_count: u32,
    pub turn_hook_count: u32,
    pub turn_classifier_count: u32,

    // -- timestamps --
    pub start_time_epoch_ms: u64,
    pub last_interaction_epoch_ms: u64,
    pub last_api_completion_epoch_ms: Option<u64>,

    // -- model --
    pub main_loop_model_override: Option<String>,
    pub initial_main_loop_model: Option<String>,
    pub is_interactive: bool,

    // -- session flags (not persisted) --
    pub session_bypass_permissions_mode: bool,
    pub session_trust_accepted: bool,
    pub session_persistence_disabled: bool,
    pub has_exited_plan_mode: bool,
    pub needs_plan_mode_exit_attachment: bool,
    pub needs_auto_mode_exit_attachment: bool,
    pub is_remote_mode: bool,
    pub scheduled_tasks_enabled: bool,

    // -- caches --
    pub cached_claude_md_content: Option<String>,
    pub system_prompt_section_cache: HashMap<String, Option<String>>,
    pub last_emitted_date: Option<String>,
    pub pending_post_compaction: bool,

    // -- plugin / skill tracking --
    pub inline_plugins: Vec<String>,
    pub invoked_skills: HashMap<String, InvokedSkill>,

    // -- error log --
    pub in_memory_error_log: Vec<ErrorEntry>,

    // -- prompt correlation --
    pub prompt_id: Option<String>,
    pub last_main_request_id: Option<String>,

    // -- agent swarm --
    pub session_created_teams: Vec<String>,

    // -- extra context directories added via /add-dir --
    pub session_extra_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokedSkill {
    pub skill_name: String,
    pub skill_path: String,
    pub content: String,
    pub invoked_at_epoch_ms: u64,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEntry {
    pub error: String,
    pub timestamp: String,
}

impl SessionState {
    pub fn new(cwd: PathBuf) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            session_id: SessionId::new(),
            parent_session_id: None,
            original_cwd: cwd.clone(),
            project_root: cwd.clone(),
            cwd,
            total_cost_usd: 0.0,
            total_api_duration_ms: 0,
            total_api_duration_without_retries_ms: 0,
            total_tool_duration_ms: 0,
            total_lines_added: 0,
            total_lines_removed: 0,
            has_unknown_model_cost: false,
            model_usage: HashMap::new(),
            turn_hook_duration_ms: 0,
            turn_tool_duration_ms: 0,
            turn_classifier_duration_ms: 0,
            turn_tool_count: 0,
            turn_hook_count: 0,
            turn_classifier_count: 0,
            start_time_epoch_ms: now_ms,
            last_interaction_epoch_ms: now_ms,
            last_api_completion_epoch_ms: None,
            main_loop_model_override: None,
            initial_main_loop_model: None,
            is_interactive: true,
            session_bypass_permissions_mode: false,
            session_trust_accepted: false,
            session_persistence_disabled: false,
            has_exited_plan_mode: false,
            needs_plan_mode_exit_attachment: false,
            needs_auto_mode_exit_attachment: false,
            is_remote_mode: false,
            scheduled_tasks_enabled: false,
            cached_claude_md_content: None,
            system_prompt_section_cache: HashMap::new(),
            last_emitted_date: None,
            pending_post_compaction: false,
            inline_plugins: Vec::new(),
            invoked_skills: HashMap::new(),
            in_memory_error_log: Vec::new(),
            prompt_id: None,
            last_main_request_id: None,
            session_created_teams: Vec::new(),
            session_extra_dirs: Vec::new(),
        }
    }

    /// Reset turn-level counters (called at the start of each agent turn).
    pub fn reset_turn_counters(&mut self) {
        self.turn_hook_duration_ms = 0;
        self.turn_tool_duration_ms = 0;
        self.turn_classifier_duration_ms = 0;
        self.turn_tool_count = 0;
        self.turn_hook_count = 0;
        self.turn_classifier_count = 0;
    }

    /// Accumulate usage for a model.
    pub fn accumulate_usage(
        &mut self,
        model_name: &str,
        input_tokens: u64,
        output_tokens: u64,
        cache_creation: u64,
        cache_read: u64,
        cost_usd: f64,
    ) {
        let entry = self.model_usage.entry(model_name.to_string()).or_default();
        entry.input_tokens += input_tokens;
        entry.output_tokens += output_tokens;
        entry.cache_creation_input_tokens += cache_creation;
        entry.cache_read_input_tokens += cache_read;
        entry.request_count += 1;
        entry.total_cost_usd += cost_usd;
        self.total_cost_usd += cost_usd;
    }

    /// Record lines changed.
    pub fn add_lines_changed(&mut self, added: u64, removed: u64) {
        self.total_lines_added += added;
        self.total_lines_removed += removed;
    }
}

// ---------------------------------------------------------------------------
// BootstrapState — the thread-safe wrapper
// ---------------------------------------------------------------------------

/// Thread-safe process-wide bootstrap state.
#[derive(Debug, Clone)]
pub struct BootstrapState {
    inner: Arc<RwLock<SessionState>>,
    boot_instant: Instant,
}

impl BootstrapState {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SessionState::new(cwd))),
            boot_instant: Instant::now(),
        }
    }

    /// Read access to the session state.
    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&SessionState) -> R,
    {
        let guard = self
            .inner
            .read()
            .expect("bootstrap state read lock poisoned");
        f(&guard)
    }

    /// Write access to the session state.
    pub fn write<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut SessionState) -> R,
    {
        let mut guard = self
            .inner
            .write()
            .expect("bootstrap state write lock poisoned");
        f(&mut guard)
    }

    /// Total wall-clock duration since bootstrap.
    pub fn total_duration(&self) -> std::time::Duration {
        self.boot_instant.elapsed()
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------

static GLOBAL_STATE: OnceLock<BootstrapState> = OnceLock::new();

/// Initialise the global bootstrap state. Must be called once from main.
pub fn init_state(cwd: PathBuf) -> &'static BootstrapState {
    GLOBAL_STATE.get_or_init(|| BootstrapState::new(cwd))
}

/// Access the global bootstrap state (panics if not yet initialised).
pub fn get_state() -> &'static BootstrapState {
    GLOBAL_STATE
        .get()
        .expect("bootstrap state not initialised — call init_state() first")
}

/// Reset for test isolation (replaces inner state).
pub fn reset_state(cwd: PathBuf) {
    if let Some(bs) = GLOBAL_STATE.get() {
        let mut guard = bs.inner.write().expect("reset lock poisoned");
        *guard = SessionState::new(cwd);
    }
}

// ---------------------------------------------------------------------------
// Convenience free-standing accessors (match claude-code's exported fns)
// ---------------------------------------------------------------------------

pub fn get_session_id() -> SessionId {
    get_state().read(|s| s.session_id.clone())
}

pub fn get_project_root() -> PathBuf {
    get_state().read(|s| s.project_root.clone())
}

pub fn get_cwd() -> PathBuf {
    get_state().read(|s| s.cwd.clone())
}

pub fn set_cwd(cwd: PathBuf) {
    get_state().write(|s| {
        s.cwd = cwd;
    });
}
