// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Stage-1 home for primitives extracted from [`crate::agent::loop_::run`],
//! the 2 343-LOC interactive CLI entry point.
//!
//! ## Migration roadmap
//!
//! * **Stage 1 (this PR, M10)** — introduce the module skeleton and a
//!   small number of genuinely reusable helpers (observer/runtime/memory
//!   wiring, workspace-dir resolution, common header banners).  The
//!   giant `run` function *calls* these helpers but still lives in
//!   `loop_.rs`; we do not delete the original body.
//! * **Stage 2 (M10.follow)** — move the stdin read loop (`loop_body`),
//!   slash-command dispatch, and tick-interval bookkeeping.
//! * **Stage 3** — hook cleanup, state persistence, and final `run`
//!   becomes a ~200-line orchestration shell.
//!
//! The staging is deliberate: the function is extensively used by
//! `entrypoints/headless.rs`, `entrypoints/cli.rs`, and the GUI/TUI
//! bridge fallback paths.  A big-bang rewrite risks regressions in
//! plan-mode / approval / cancellation flows.  Every Stage N
//! extraction ships with parity tests proving bytewise-identical
//! behaviour on a fake provider.
//!
//! ## Design constraints
//!
//! * No new async state machines; keep all helpers functional.
//! * Never swallow errors — return `anyhow::Result` and let `run`
//!   contextualise.
//! * Helpers must be stateless or take `&Config` by reference; no
//!   owned `Config` copies (memory cost).

pub mod wiring;

pub use wiring::{build_memory, build_observer, build_runtime, build_security};
