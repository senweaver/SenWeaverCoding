// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Webhook resource — inbound channel webhook handlers.
//!
//! D2.4 placeholder for the upcoming handler move from
//! [`crate::gateway::api`]:
//!
//! * `handle_claude_code_hook`
//! * Per-channel webhook routes (Slack `/events`, Telegram
//!   `/telegram/<token>`, Lark `/lark/event`, WeChat `/wechat`, …).
//!
//! These handlers share a common shape (signature verification +
//! channel dispatcher); extracting them here means the signature-
//! verification middleware can be mounted on the whole sub-router
//! in one place (D6.3 secret scanning inbound).

use axum::Router;

pub fn webhook_router() -> Router {
    Router::new()
}
