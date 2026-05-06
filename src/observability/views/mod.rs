// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Pure (UI-framework-free) view models for the multi-agent
//! observability panels.
//!
//! These types are consumed by the TUI renderers in
//! `crate::tui::panels` and by any future surface (the Tauri-hosted
//! desktop UI, gateway JSON endpoints, downstream tooling) that needs
//! the same shape.  The view models intentionally live outside of any
//! UI framework so they compile unconditionally.

pub mod budget;
pub mod provider_health;

pub use budget::{BudgetRow, BudgetView};
pub use provider_health::{ProviderHealthRow, ProviderHealthView};
