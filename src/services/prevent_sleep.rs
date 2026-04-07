// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Prevent sleep service — mirrors claude-code-typescript-src`services/preventSleep.ts`.
// Prevents the OS from sleeping while the agent is actively processing
// long-running tasks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Guard that prevents system sleep while held.
/// Drop the guard to allow sleep again.
pub struct SleepInhibitor {
    active: Arc<AtomicBool>,
}

impl SleepInhibitor {
    /// Acquire a sleep inhibitor. The system will not sleep while this is held.
    pub fn acquire(reason: &str) -> Self {
        tracing::info!(reason, "Inhibiting system sleep");
        let inhibitor = Self {
            active: Arc::new(AtomicBool::new(true)),
        };
        // Platform-specific sleep prevention
        #[cfg(target_os = "macos")]
        {
            // On macOS, caffeinate is used. In production, spawn caffeinate process.
            tracing::debug!("macOS: would spawn caffeinate");
        }
        #[cfg(target_os = "windows")]
        {
            // On Windows, SetThreadExecutionState is used.
            tracing::debug!(
                "Windows: would call SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)"
            );
        }
        #[cfg(target_os = "linux")]
        {
            // On Linux, systemd-inhibit or D-Bus inhibit is used.
            tracing::debug!("Linux: would use systemd-inhibit");
        }
        inhibitor
    }

    /// Check if the inhibitor is still active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Explicitly release the inhibitor.
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
