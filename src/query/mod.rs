// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Query engine module — mirrors claude-code's `query/` and `query.ts`.
//
// Provides query configuration, token budget management, dependency
// injection for queries, and stop-hook evaluation.

pub mod compact;
pub mod config;
pub mod deps;
pub mod engine;
pub mod stop_hooks;
pub mod token_budget;

pub use compact::{
    CompactionConfig, CompactionStrategy, create_collapse_marker, should_compact,
    sliding_window_compact,
};
pub use config::QueryConfig;
pub use deps::QueryDeps;
pub use engine::QueryEngine;
pub use stop_hooks::{StopHook, StopHookResult, standard_stop_hooks};
pub use token_budget::{TokenBudget, estimate_tokens};
