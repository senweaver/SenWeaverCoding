// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
//! Language Server Protocol orchestration layer.
//!
//! This module sits between the persisted [`crate::config::schema::LspConfig`]
//! and the low-level stdio JSON-RPC client in [`crate::services::lsp`].
//! Responsibilities:
//!
//! - **Reconcile**: when the config changes, start/stop/restart the LSP
//!   server processes that should be running.  Driven by
//!   [`crate::config::live::LiveConfig`] hot-swaps so users editing the
//!   desktop Settings page see effects without restarting the gateway.
//! - **Install**: download and unpack managed binaries (rust-analyzer,
//!   typescript-language-server, pyright) into
//!   `~/.senweavercoding/lsp/<id>/` and persist the resulting paths into
//!   [`crate::config::schema::LspInstallState::Installed`].
//! - **Bridge diagnostics**: subscribe each running server to the gateway
//!   broadcast channel so the desktop UI receives `publishDiagnostics`
//!   notifications in real time.
//!
//! The module is wired into [`crate::gateway::AppState`] so HTTP routes
//! and the WebSocket handler can both reach it.

pub mod events;
pub mod installer;
pub mod manager;

pub use events::{InstallPhase, LspBroadcast, LspBroadcastEvent, ServerLifecycleStatus};
pub use installer::{InstallProgress, InstallReport, install};
pub use manager::LspManager;
