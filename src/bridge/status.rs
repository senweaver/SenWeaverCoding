// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::types::BridgeStatus;

pub fn status_label(status: BridgeStatus) -> &'static str {
    match status {
        BridgeStatus::Disconnected => "Disconnected",
        BridgeStatus::Connecting => "Connecting…",
        BridgeStatus::Connected => "Connected",
        BridgeStatus::Paired => "Paired",
        BridgeStatus::Error => "Error",
    }
}

pub fn status_indicator(status: BridgeStatus) -> &'static str {
    match status {
        BridgeStatus::Disconnected => "○",
        BridgeStatus::Connecting => "◌",
        BridgeStatus::Connected => "●",
        BridgeStatus::Paired => "◉",
        BridgeStatus::Error => "✗",
    }
}

pub fn is_usable(status: BridgeStatus) -> bool {
    matches!(status, BridgeStatus::Connected | BridgeStatus::Paired)
}
