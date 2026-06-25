// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use super::checkpoint::CheckpointStore;
use super::traits::AgentHandle;
use crate::agent::self_assess::critic::CriticContext;

const DEFAULT_FLOW_SCOPE: &str = "__global__";

static AGENT_HANDLES: OnceLock<RwLock<HashMap<String, Arc<dyn AgentHandle>>>> = OnceLock::new();
static GLOBAL_CHECKPOINT_STORE: OnceLock<Arc<CheckpointStore>> = OnceLock::new();
static CRITIC_CONTEXTS: OnceLock<RwLock<HashMap<String, CriticContext>>> = OnceLock::new();

fn handles() -> &'static RwLock<HashMap<String, Arc<dyn AgentHandle>>> {
    AGENT_HANDLES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn critic_contexts() -> &'static RwLock<HashMap<String, CriticContext>> {
    CRITIC_CONTEXTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn current_scope() -> String {
    match crate::session::current_session_context() {
        Some(ctx) if !ctx.session_id.is_empty() => ctx.session_id,
        _ => DEFAULT_FLOW_SCOPE.to_string(),
    }
}

pub fn set_global_agent_handle(handle: Arc<dyn AgentHandle>) {
    let scope = current_scope();
    let cell = handles();
    let mut guard = cell.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.insert(scope, handle);
}

pub fn global_agent_handle() -> Option<Arc<dyn AgentHandle>> {
    let cell = handles();
    let guard = cell.read().ok()?;
    let scope = current_scope();
    if let Some(handle) = guard.get(&scope) {
        return Some(handle.clone());
    }
    guard.get(DEFAULT_FLOW_SCOPE).cloned()
}

#[doc(hidden)]
pub fn clear_global_agent_handle_for_tests() {
    if let Some(cell) = AGENT_HANDLES.get() {
        let _ = cell.write().map(|mut g| g.clear());
    }
}

pub fn set_global_critic_context(ctx: CriticContext) {
    let scope = current_scope();
    let cell = critic_contexts();
    let mut guard = cell.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.insert(scope, ctx);
}

pub fn global_critic_context() -> Option<CriticContext> {
    let cell = critic_contexts();
    let guard = cell.read().ok()?;
    let scope = current_scope();
    if let Some(ctx) = guard.get(&scope) {
        return Some(ctx.clone());
    }
    guard.get(DEFAULT_FLOW_SCOPE).cloned()
}

pub fn global_checkpoint_store() -> Arc<CheckpointStore> {
    GLOBAL_CHECKPOINT_STORE
        .get_or_init(|| Arc::new(CheckpointStore::default()))
        .clone()
}
