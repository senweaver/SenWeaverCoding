// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod store;
pub mod types;

pub use types::*;

use parking_lot::Mutex;
use std::sync::OnceLock;

static TRUST_TRACKER: OnceLock<Mutex<TrustTracker>> = OnceLock::new();

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
    if approved {
        tracker.record_success(tool_name);
    } else {
        tracker.record_correction(tool_name, CorrectionType::UserOverride, description);
    }
}

pub fn domain_regressed(domain: &str) -> bool {
    global_tracker().lock().check_regression(domain).is_some()
}
