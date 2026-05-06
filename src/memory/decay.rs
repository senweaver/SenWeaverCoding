// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{MemoryCategory, MemoryEntry};
use chrono::{DateTime, Utc};

pub const DEFAULT_HALF_LIFE_DAYS: f64 = 7.0;

pub fn apply_time_decay(entries: &mut [MemoryEntry], half_life_days: f64) {
    let half_life = if half_life_days <= 0.0 {
        DEFAULT_HALF_LIFE_DAYS
    } else {
        half_life_days
    };

    let now = Utc::now();

    for entry in entries.iter_mut() {

        if entry.category == MemoryCategory::Core {
            continue;
        }

        let score = match entry.score {
            Some(s) => s,
            None => continue,
        };

        let ts = match DateTime::parse_from_rfc3339(&entry.timestamp) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => continue,
        };

        let age_days = now.signed_duration_since(ts).num_seconds().max(0) as f64 / 86_400.0;

        let decay_factor = (-age_days / half_life * std::f64::consts::LN_2).exp();
        entry.score = Some(score * decay_factor);
    }
}
