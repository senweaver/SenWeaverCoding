// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Event-triggered automation (routines engine).
//!
//! Routines are lightweight automation rules that match incoming events (from
//! channels, cron, webhooks, or system signals) using configurable pattern
//! strategies (exact, glob, regex) and fire actions (SOP triggers, shell
//! commands, messages, cron jobs).  Each routine supports per-routine cooldown
//! to prevent rapid re-triggering.
//!
//! ## Loading
//!
//! Routines are defined in `routines.toml` in the workspace root:
//!
//! ```toml
//! [[routines]]
//! name = "deploy-notify"
//! description = "Notify Slack on deploy webhook"
//! cooldown_secs = 60
//!
//! [[routines.patterns]]
//! source = "webhook"
//! pattern = "/api/deploy"
//! strategy = "exact"
//!
//! [routines.action]
//! type = "message"
//! channel = "slack-general"
//! text = "Deploy triggered!"
//! ```

pub mod engine;
pub mod event_matcher;

#[allow(unused_imports)]
pub use engine::{
    Routine, RoutineAction, RoutineDispatchResult, RoutinesEngine, load_routines,
    load_routines_from_file,
};
#[allow(unused_imports)]
pub use event_matcher::{EventPattern, MatchStrategy, RoutineEvent, matches, matches_any};
