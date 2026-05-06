// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! `LiveConfig` — the agent-facing zero-cost config read API.
//!
//! This module re-exports [`crate::config::hot_reload::LiveConfig`] so that
//! code that needs the agent hot-path API can import it as `crate::config::live::LiveConfig`
//! without knowing about the underlying `ArcSwap` implementation.
pub use crate::config::hot_reload::LiveConfig;
