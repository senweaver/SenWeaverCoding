// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

enum PlatformGuard {
    #[cfg(target_os = "windows")]
    ExecutionState(u32),
    #[cfg(unix)]
    Child(std::process::Child),
}

static PLATFORM_GUARD: Mutex<Option<PlatformGuard>> = Mutex::new(None);

pub struct SleepInhibitor {
    active: AtomicBool,
}

impl SleepInhibitor {
    pub fn acquire(reason: &str) -> Self {
        let acquired = acquire_platform_guard(reason);
        let inhibitor = Self {
            active: AtomicBool::new(acquired),
        };
        if acquired {
            tracing::info!(reason, "System sleep inhibitor active");
        } else {
            tracing::warn!(
                reason,
                "System sleep inhibitor could not be acquired; the host may suspend during long agent turns"
            );
        }
        inhibitor
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn release(&self) {
        if self.active.swap(false, Ordering::Relaxed) {
            release_platform_guard();
            tracing::info!("System sleep inhibitor released");
        }
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        self.release();
    }
}

fn acquire_platform_guard(reason: &str) -> bool {
    let mut slot = PLATFORM_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Power::{
            ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
        };
        let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        if previous == 0 {
            tracing::warn!(
                reason,
                "SetThreadExecutionState failed; Windows may still suspend during agent turns"
            );
            return false;
        }
        *slot = Some(PlatformGuard::ExecutionState(previous));
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        let mut cmd = crate::util::hidden_sync_command("caffeinate");
        cmd.arg("-i");
        match cmd.spawn() {
            Ok(child) => {
                *slot = Some(PlatformGuard::Child(child));
                return true;
            }
            Err(err) => {
                tracing::warn!(
                    reason,
                    error = %err,
                    "failed to spawn caffeinate; macOS may still sleep during agent turns"
                );
                return false;
            }
        }
    }

    #[cfg(all(target_os = "linux", not(target_os = "macos")))]
    {
        let mut cmd = crate::util::hidden_sync_command("systemd-inhibit");
        cmd.args([
            "--what=sleep:idle",
            "--who=SenWeaverCoding",
            "--why=Agent turn in progress",
            "--mode=block",
            "sleep",
            "infinity",
        ]);
        match cmd.spawn() {
            Ok(child) => {
                *slot = Some(PlatformGuard::Child(child));
                return true;
            }
            Err(err) => {
                tracing::warn!(
                    reason,
                    error = %err,
                    "failed to spawn systemd-inhibit; Linux host may still suspend during agent turns"
                );
                return false;
            }
        }
    }

    #[cfg(not(any(target_os = "windows", unix)))]
    {
        let _ = reason;
        tracing::warn!(
            reason,
            "no platform sleep inhibitor available on this OS"
        );
        false
    }
}

fn release_platform_guard() {
    let mut slot = PLATFORM_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let Some(guard) = slot.take() else {
        return;
    };

    match guard {
        #[cfg(target_os = "windows")]
        PlatformGuard::ExecutionState(previous) => {
            use windows_sys::Win32::System::Power::SetThreadExecutionState;
            let restored = unsafe { SetThreadExecutionState(previous) };
            if restored == 0 {
                tracing::warn!(
                    target: "prevent_sleep",
                    "failed to restore previous thread execution state; system sleep policy may stay overridden until process exit"
                );
            }
        }
        #[cfg(unix)]
        PlatformGuard::Child(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
