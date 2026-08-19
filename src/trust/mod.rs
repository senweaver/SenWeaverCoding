// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod store;
pub mod types;

pub use types::*;

use parking_lot::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

static TRUST_TRACKER: OnceLock<Mutex<TrustTracker>> = OnceLock::new();

static LAST_DECAY_EPOCH_SECS: AtomicU64 = AtomicU64::new(0);
const DECAY_THROTTLE_SECS: u64 = 3600;

fn maybe_apply_decay(tracker: &mut TrustTracker) {
    let now = chrono::Utc::now();
    let now_secs = now.timestamp().max(0) as u64;
    let last = LAST_DECAY_EPOCH_SECS.load(Ordering::Relaxed);
    if now_secs.saturating_sub(last) >= DECAY_THROTTLE_SECS {
        LAST_DECAY_EPOCH_SECS.store(now_secs, Ordering::Relaxed);
        tracker.apply_decay(now);
    }
}

pub fn global_tracker() -> &'static Mutex<TrustTracker> {
    TRUST_TRACKER.get_or_init(|| {
        let (config, dir) = match crate::services::try_get_services() {
            Some(svc) => {
                let cfg = svc.config();
                (cfg.trust.clone(), cfg.workspace_dir.join(".sen").join("trust"))
            }
            None => (
                TrustConfig::default(),
                std::env::temp_dir().join("sen-trust"),
            ),
        };
        Mutex::new(TrustTracker::new_persistent(config, &dir))
    })
}

pub fn record_tool_decision(tool_name: &str, approved: bool, description: &str) {
    let mut tracker = global_tracker().lock();
    maybe_apply_decay(&mut tracker);
    if approved {
        tracker.record_success(tool_name);
    } else {
        tracker.record_correction(tool_name, CorrectionType::UserOverride, description);
    }
}

pub fn domain_regressed(domain: &str) -> bool {
    let mut tracker = global_tracker().lock();
    maybe_apply_decay(&mut tracker);
    tracker.check_regression(domain).is_some()
}
