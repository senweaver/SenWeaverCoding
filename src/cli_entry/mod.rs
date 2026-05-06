// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! extraction target for the `src/main.rs` monolith.
//!
//! `src/main.rs` currently hosts the full CLI argument parser
//! (clap-derive), the top-level `async fn main()` dispatcher, and the
//! per-subcommand handlers — together ~4k lines.  The plan
//! decomposes this into `bin/sen_cli/{cli_args,startup,commands/*}`.
//!
//! introduces `cli_entry` as a *library-side* staging area so
//! the extraction can proceed incrementally: helpers that are safe to
//! move (no clap-derive dependency) land here first, then the clap
//! surface migrates behind a feature flag, and finally `main.rs`
//! collapses to a minimal `fn main() { cli_entry::run() }` entry.
//!
//! Sub-modules landed in this sprint:
//! * [`bootstrap`] — `.env` discovery, tracing init helpers, and the
//!   "no-command" help printer.  All three are invoked from `main.rs`
//!   today (real call sites).
//!
//! Modules staged for the follow-up sprint:
//! * `cli_args` — clap-derive argument types.
//! * `startup` — config load + service container init.
//! * `commands::{start, config, session, memory, hardware}` — per-
//!   subcommand handlers.

pub mod bootstrap;
