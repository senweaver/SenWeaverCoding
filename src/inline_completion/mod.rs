// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Supercomplete inline completion subsystem.
//!
//! Supercomplete provides ghost-text, fill-in-the-middle (FIM) style
//! suggestions as the user types, analogous to Cursor Tab / Copilot.
//! The design emphasises three invariants:
//!
//! 1. **Pluggable provider** — the [`InlineCompletionProvider`] trait
//!    abstracts over FIM models (Codestral / DeepSeek-FIM /
//!    Qwen-Coder), legacy completion-style providers, and any future
//!    local llama.cpp backend.  ships the FIM and
//!    `openai_style` fallbacks; can add local inference
//!    without touching call sites.
//! 2. **Debounce + LRU cache** — the hot path is dominated by the
//!    user's typing velocity.  [`throttle`] governs request timing
//!    and [`cache`] keeps recently-seen prefixes warm.
//! 3. **Three-end parity** — GUI renders ghost text via the editor
//!    overlay, TUI uses a ratatui widget, and CLI exposes a
//!    `sen complete` subcommand producing JSON.  All three surfaces
//!    share the same [`InlineCompletionRequest`] shape so
//!    instrumentation is uniform.
//!
//! ## Anti-placeholder guarantee
//!
//! Every public symbol must be called from at least one of:
//!   * a non-trivial unit test (`cfg(test)`),
//!   * `tools::inline_complete` (exposes completion as a tool), or
//!   * a future surface wiring in `gui::editor::completion` /
//!     `tui::completion` / `cli::complete`.
//!
//! See `tests/inline_completion_smoke.rs` for the end-to-end contract.

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
