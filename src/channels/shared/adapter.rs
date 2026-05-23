// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub fn normalize_channel_id(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}
