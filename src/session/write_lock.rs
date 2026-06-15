// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const STALE_AFTER: Duration = Duration::from_secs(120);

const TOUCH_FAILURE_DEGRADE_THRESHOLD: u64 = 3;

pub struct SessionWriteLock {
    path: PathBuf,
    touch_failures: AtomicU64,
    degraded: AtomicBool,
}

impl SessionWriteLock {
    pub fn acquire(lock_path: &Path) -> std::io::Result<Option<Self>> {
        for attempt in 0..2 {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(lock_path)
            {
                Ok(mut file) => {
                    let _ = file.write_all(lock_contents().as_bytes());
                    let _ = file.sync_all();
                    return Ok(Some(Self {
                        path: lock_path.to_path_buf(),
                        touch_failures: AtomicU64::new(0),
                        degraded: AtomicBool::new(false),
                    }));
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt == 0 && is_stale(lock_path) {
                        tracing::warn!(
                            lock = %lock_path.display(),
                            "stale session write lock detected; taking over"
                        );
                        let _ = std::fs::remove_file(lock_path);
                        continue;
                    }
                    return Ok(None);
                }
                Err(err) => return Err(err),
            }
        }
        Ok(None)
    }

    pub fn touch(&self) {
        let result = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(lock_contents().as_bytes()));
        match result {
            Ok(()) => {
                self.touch_failures.store(0, Ordering::Relaxed);
                self.degraded.store(false, Ordering::Relaxed);
            }
            Err(err) => {
                let failures = self.touch_failures.fetch_add(1, Ordering::Relaxed) + 1;
                if failures >= TOUCH_FAILURE_DEGRADE_THRESHOLD
                    && !self.degraded.swap(true, Ordering::Relaxed)
                {
                    tracing::error!(
                        lock = %self.path.display(),
                        consecutive_failures = failures,
                        error = %err,
                        "session write lock heartbeat (touch) failing repeatedly; lock mtime is stale but liveness now relies on PID detection"
                    );
                }
            }
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Relaxed)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SessionWriteLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_contents() -> String {
    let started_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("pid={}\nstarted_at_ms={}\n", std::process::id(), started_ms)
}

fn is_stale(lock_path: &Path) -> bool {
    #[cfg(unix)]
    {
        if let Some(pid) = read_lock_pid(lock_path) {
            if pid == std::process::id() {
                return false;
            }
            return !unix_pid_alive(pid);
        }
    }
    #[cfg(windows)]
    {
        if let Some(pid) = read_lock_pid(lock_path) {
            if pid == std::process::id() {
                return false;
            }
            return !windows_pid_alive(pid);
        }
    }
    std::fs::metadata(lock_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| age > STALE_AFTER)
        .unwrap_or(false)
}

#[cfg(any(unix, windows))]
fn read_lock_pid(lock_path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(lock_path).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|raw| raw.trim().parse::<u32>().ok())
}

#[cfg(unix)]
#[allow(clippy::cast_possible_wrap)]
fn unix_pid_alive(pid: u32) -> bool {
    let res = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if res == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn windows_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, GetLastError};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const STILL_ACTIVE_CODE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut exit_code: u32 = 0;
        let queried = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        if queried == 0 {
            return true;
        }
        exit_code == STILL_ACTIVE_CODE
    }
}
