// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! target location for shared channel plumbing.
//!
//! `channels/mod.rs` (11k+ lines) and per-platform adapters such as
//! `channels/telegram.rs` (5k+ lines), `channels/slack.rs`,
//! `channels/discord.rs`, `channels/lark.rs` duplicate the same five
//! concerns at varying levels of quality:
//!
//! * **adapter** — trait implementations that register the channel.
//! * **message_format** — platform-specific Markdown / rich-text
//!   escape rules and converters.
//! * **media** — download / upload / caption helpers for images,
//!   audio, video, documents.
//! * **auth** — HMAC / token / OAuth verification on inbound
//!   webhooks.
//! * **webhook_common** — Axum extractors for signed payloads,
//!   replay protection, rate limiting per channel.
//!
//! The follow-up sprint lifts the shared logic out of each
//! channel into the sub-modules below.  Today we stage the module
//! tree so new code can land in `shared::*` directly.

pub mod adapter;
pub mod auth;
pub mod media;
pub mod message_format;
pub mod webhook_common;
