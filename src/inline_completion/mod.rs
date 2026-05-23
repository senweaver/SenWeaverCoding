// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod cache;
pub mod context_builder;
pub mod ghost_renderer;

pub mod nep;
pub mod providers;
pub mod registry;
pub mod stats;
pub mod throttle;
pub mod traits;

pub use cache::{CacheKey, CompletionCache};
pub use context_builder::{InlineContext, build_context_from_window};
pub use ghost_renderer::{GhostText, GhostTextRenderer};
pub use registry::{InlineCompletionRegistry, RegistryHandle};
pub use stats::{AcceptanceEvent, CompletionStats, global_stats};
pub use throttle::{Throttler, ThrottlerDecision};
pub use traits::{
    CompletionStream, InlineCompletionError, InlineCompletionProvider, InlineCompletionRequest,
    InlineCompletionResponse, Language, Suggestion,
};
