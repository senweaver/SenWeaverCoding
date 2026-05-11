// Copyright (C) SenWeaverCoding contributors. Licensed under the Apache-2.0
// license.  See the LICENSE file in the workspace root for details.

use std::sync::OnceLock;
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

                tracing::info!(
                    "[sen-desktop] AssignProcessToJobObject failed ({err}); likely already in a job, relying on graceful shutdown for child cleanup"
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

static SHUTDOWN_LATCH: OnceLock<()> = OnceLock::new();

pub fn run_full_shutdown(app: &AppHandle, deadline: Duration) {
    if SHUTDOWN_LATCH.set(()).is_err() {
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

    #[cfg(unix)]
    {

        signal_process_group(libc::SIGTERM);
    }

    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        "[sen-desktop] coordinated shutdown sequence finished"
    );
}
