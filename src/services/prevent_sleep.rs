// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct SleepInhibitor {
    active: Arc<AtomicBool>,
}

impl SleepInhibitor {

    pub fn acquire(reason: &str) -> Self {
        tracing::info!(reason, "Inhibiting system sleep");
        let inhibitor = Self {
            active: Arc::new(AtomicBool::new(true)),
        };

        #[cfg(target_os = "macos")]
        {

            tracing::debug!("macOS: would spawn caffeinate");
        }
        #[cfg(target_os = "windows")]
        {

            tracing::debug!(
                "Windows: would call SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)"
            );
        }
        #[cfg(target_os = "linux")]
        {

            tracing::debug!("Linux: would use systemd-inhibit");
        }
        inhibitor
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn release(&self) {
        if self.active.swap(false, Ordering::Relaxed) {
            tracing::info!("System sleep inhibitor released");
        }
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        self.release();
    }
}
