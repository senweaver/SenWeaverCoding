// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Channel adapter helpers shared across every concrete channel.
//!
//! The authoritative `ChannelAdapter` trait lives in
//! [`crate::channels::traits`]; this module surfaces thin helpers that
//! every adapter would otherwise have to reinvent.

pub fn normalize_channel_id(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}
