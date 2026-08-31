// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod engine;
pub mod engines;
pub mod health;
pub mod parsers;
pub mod provider_routing;
pub mod ranker;
pub mod routing;
pub mod tool;

pub use engine::{
    ApiKeys, SearchCategory, SearchContext, SearchEngine, SearchHit, TimeRange,
    default_user_agent, pick_rotating_user_agent,
};
pub use ranker::{merge_and_dedup, render_results_markdown};
pub use routing::{EngineRegistry, global_registry, known_aliases, known_engine_ids};
