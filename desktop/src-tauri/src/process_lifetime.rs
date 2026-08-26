// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::AppHandle;

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, SetInformationJobObject,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::core::PCWSTR;

    struct JobOwner(HANDLE);

    unsafe impl Send for JobOwner {}
    unsafe impl Sync for JobOwner {}

    impl Drop for JobOwner {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    static JOB: OnceLock<JobOwner> = OnceLock::new();

    pub fn install_kill_on_close_job() {
        if JOB.get().is_some() {
            return;
        }
        unsafe {
            let job: HANDLE = match CreateJobObjectW(None, PCWSTR::null()) {
                Ok(h) if !h.is_invalid() => h,
                Ok(_) => {
                    tracing::warn!(
                        "[sen-desktop] CreateJobObjectW returned invalid handle; child processes will not be auto-killed on app exit"
                    );
                    return;
                }
                Err(err) => {
                    tracing::warn!(
                        "[sen-desktop] CreateJobObjectW failed ({err}); child processes will not be auto-killed on app exit"
                    );
                    return;
                }
            };

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            let basic = JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_BREAKAWAY_OK
                    | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
                ..std::mem::zeroed()
            };
            info.BasicLimitInformation = basic;

            let info_ptr = (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast();
            let info_size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            if let Err(err) = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                info_ptr,
                info_size,
            ) {
                tracing::warn!(
                    "[sen-desktop] SetInformationJobObject failed ({err}); child processes may survive app exit"
                );
                let _ = CloseHandle(job);
                return;
            }

            if let Err(err) = AssignProcessToJobObject(job, GetCurrentProcess()) {

                tracing::warn!(
                    "[sen-desktop] AssignProcessToJobObject failed ({err}); child processes will NOT be auto-killed by the OS on abnormal exit. \
                     Graceful shutdown will explicitly kill known children (gateway child, terminal shells) as a fallback"
                );
                let _ = CloseHandle(job);
                return;
            }

            let _ = JOB.set(JobOwner(job));
            tracing::info!(
                "[sen-desktop] Win32 JobObject installed: every child process will be killed on app exit"
            );
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::sync::OnceLock;

    static INSTALLED: OnceLock<()> = OnceLock::new();

    pub fn install_kill_on_close_job() {
        if INSTALLED.get().is_some() {
            return;
        }

        unsafe {
            let r = libc::setpgid(0, 0);
            if r != 0 {
                let err = std::io::Error::last_os_error();
                tracing::info!(
                    "[sen-desktop] setpgid(0,0) failed ({err}); child processes may survive app exit on Unix"
                );
                return;
            }
        }
        let _ = INSTALLED.set(());
        tracing::info!(
            "[sen-desktop] Unix process group installed: SIGTERM/SIGKILL will be broadcast to children on app exit"
        );
    }
}

#[cfg(not(any(target_os = "windows", unix)))]
mod platform {
    pub fn install_kill_on_close_job() {}
}

#[cfg(unix)]
fn signal_process_group(sig: libc::c_int) {
    unsafe {
        let pgid = libc::getpgrp();
        if pgid > 0 {
            let _ = libc::killpg(pgid, sig);
        }
    }
}

pub fn install_kill_on_close_job() {
    platform::install_kill_on_close_job();
}

#[cfg(target_os = "windows")]
mod singleton {
    use std::sync::OnceLock;

    use windows::core::w;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, SetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_ABANDONED,
        WAIT_OBJECT_0, WIN32_ERROR,
    };
    use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};

    struct MutexOwner(HANDLE);

    unsafe impl Send for MutexOwner {}
    unsafe impl Sync for MutexOwner {}

    impl Drop for MutexOwner {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    static MUTEX: OnceLock<MutexOwner> = OnceLock::new();

    pub fn claim_or_exit() {
        unsafe {
            SetLastError(WIN32_ERROR(0));
            let handle = match CreateMutexW(None, true, w!("Local\\com.senweaver.desktop.runtime")) {
                Ok(h) if !h.is_invalid() => h,
                _ => return,
            };
            if GetLastError() == ERROR_ALREADY_EXISTS {
                tracing::info!(
                    "[sen-desktop] another desktop process still holds the runtime lock; waiting to take over"
                );
                let wait = WaitForSingleObject(handle, 12_000);
                if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                    tracing::warn!(
                        "[sen-desktop] another desktop process is still running; exiting this duplicate instance"
                    );
                    let _ = CloseHandle(handle);
                    std::process::exit(0);
                }
            }
            let _ = MUTEX.set(MutexOwner(handle));
        }
    }
}

#[cfg(unix)]
mod singleton {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    static LOCK_FILE: OnceLock<std::fs::File> = OnceLock::new();

    pub fn claim_or_exit() {
        let path = std::env::temp_dir().join("com.senweaver.desktop.runtime.lock");
        let file = match OpenOptions::new().create(true).read(true).write(true).open(&path) {
            Ok(f) => f,
            Err(err) => {
                tracing::warn!("[sen-desktop] could not open runtime lock file ({err}); continuing without singleton lock");
                return;
            }
        };
        let fd = file.as_raw_fd();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                let _ = LOCK_FILE.set(file);
                return;
            }
            if Instant::now() >= deadline {
                tracing::warn!(
                    "[sen-desktop] another desktop process is still running; exiting this duplicate instance"
                );
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(not(any(target_os = "windows", unix)))]
mod singleton {
    pub fn claim_or_exit() {}
}

pub fn claim_singleton_or_exit() {
    singleton::claim_or_exit();
}

static SHUTDOWN_LATCH: AtomicBool = AtomicBool::new(false);

pub fn is_shutting_down() -> bool {
    SHUTDOWN_LATCH.load(Ordering::SeqCst)
}

pub fn reset_shutdown_latch() {
    SHUTDOWN_LATCH.store(false, Ordering::SeqCst);
}

pub fn run_full_shutdown(app: &AppHandle, deadline: Duration) {
    if SHUTDOWN_LATCH.swap(true, Ordering::SeqCst) {
        return;
    }

    let started = Instant::now();
    tracing::info!(
        deadline_ms = deadline.as_millis() as u64,
        "[sen-desktop] beginning coordinated shutdown of all subsystems"
    );

    let was_running = senweavercoding::gateway::is_running();
    let signaled = senweavercoding::gateway::request_shutdown();
    if signaled {
        tracing::info!("[sen-desktop] shutdown signal sent to embedded gateway; waiting for graceful drain");
    } else if was_running {
        tracing::info!("[sen-desktop] gateway running but shutdown channel not yet wired; relying on JobObject");
    } else {
        tracing::info!("[sen-desktop] embedded gateway not yet running; skipping graceful gateway drain");
    }

    crate::terminal::shutdown_all(app);

    if was_running {
        let drain_deadline = Instant::now() + deadline;
        let poll_interval = Duration::from_millis(50);
        while !senweavercoding::gateway::is_fully_stopped() {
            std::thread::sleep(poll_interval);
            if Instant::now() >= drain_deadline {
                tracing::warn!(
                    "[sen-desktop] gateway graceful shutdown exceeded {}ms deadline; proceeding with hard exit",
                    deadline.as_millis()
                );
                break;
            }
        }
        if senweavercoding::gateway::is_fully_stopped() {
            tracing::info!(
                drain_ms = started.elapsed().as_millis() as u64,
                "[sen-desktop] embedded gateway has fully stopped; resources released"
            );
        }
    }

    crate::kill_gateway_child();

    #[cfg(unix)]
    {

        signal_process_group(libc::SIGTERM);
    }

    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        "[sen-desktop] coordinated shutdown sequence finished"
    );
}
