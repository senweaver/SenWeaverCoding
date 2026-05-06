// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `turn_engine` is the new home for the agent turn
//! pipeline.  The /2 implementation lives in `agent::loop_` as a
//! single ~10k line file; this module is the target architecture that
//! decomposes it into focused components while keeping every call site
//! green (`cargo check --lib`).
//!
//! ## Sub-modules
//!
//! * [`cache_bind`] — Blackboard tool-result cache lookup / write-back
//!   helpers that used to live inline inside `loop_::execute_one_tool`.
//! * [`recovery_bind`] — `Recovery::classify_error` invocation + tracing
//!   helper used when a tool call's `Result::Err` arm needs structured
//!   classification.
//! * [`observer_bind`] — Canonical `ObserverEvent::{ToolCallStart,
//!   ToolCall}` emission helpers so all tool execution paths emit the
//!   same event shape.
//! * [`guardrails`] — RBAC pre-check + `guardrails::check_tool_guardrails`
//!   wrapper that returns a structured verdict usable by both the
//!   legacy `loop_` and future `tool_exec`.
//! * [`interrupt`] — Cancellation token helpers (wait-or-run pattern,
//!   `ToolLoopCancelled` error construction).
//! * [`tool_exec`] — Home of the forthcoming `execute_one_tool` move.
//!   For now it hosts composable building blocks used by `loop_`.
//! * [`message`] — Message-list manipulation helpers (truncation,
//!   system prompt injection, token-aware pruning entry points).
//! * [`turn`] — Turn-level façade (future home of `run_turn_inner`).
//! * [`observer_bind`] is the only sub-module that carries runtime
//!   cost; all others are pure helpers.
//!
//! ## Migration status ()
//!
//! * D2.1-a: module skeleton + cache_bind / recovery_bind / observer_bind
//!   / guardrails / interrupt helpers **landed** (2026-04-21).
//! * D2.1-b: `tool_exec::execute_one_tool` physical move — deferred to a
//!   dedicated sprint; the helpers below are already invoked by
//!   `loop_::execute_one_tool`, so the coupling is real.
//!
//! Every public symbol in this module MUST have ≥ 1 real call site
//! outside `turn_engine::` — anti-placeholder guard (plan §I).

pub mod cache_bind;
pub mod guardrails;
pub mod interrupt;
pub mod message;
pub mod observer_bind;
pub mod recovery_bind;
pub mod tool_exec;
pub mod turn;

#[doc(inline)]
pub use cache_bind::{ToolCacheEntry, try_tool_cache_hit, write_tool_cache};
#[doc(inline)]
pub use guardrails::{GuardrailVerdict, check_rbac, check_tool_guardrails};
#[doc(inline)]
pub use interrupt::{ToolRunOutcome, run_or_cancel};
#[doc(inline)]
pub use observer_bind::{emit_tool_call_end, emit_tool_call_start};
#[doc(inline)]
pub use recovery_bind::classify_and_trace;
