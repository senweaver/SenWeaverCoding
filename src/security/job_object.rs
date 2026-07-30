// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct JobLimits {

    pub process_memory_bytes: Option<u64>,

    pub job_memory_bytes: Option<u64>,

    pub max_processes: Option<u32>,

    pub cpu_rate_percent: Option<u32>,

    pub kill_on_close: bool,

    pub wall_time: Option<Duration>,

    pub cpu_time: Option<Duration>,
}

impl Default for JobLimits {
    fn default() -> Self {
        Self {
            process_memory_bytes: Some(2 * 1024 * 1024 * 1024),
            job_memory_bytes: Some(4 * 1024 * 1024 * 1024),
            max_processes: Some(64),
            cpu_rate_percent: Some(80),
            kill_on_close: true,
            wall_time: Some(Duration::from_secs(10 * 60)),
            cpu_time: None,
        }
    }
}

impl JobLimits {

    pub fn unlimited() -> Self {
        Self {
            process_memory_bytes: None,
            job_memory_bytes: None,
            max_processes: None,
            cpu_rate_percent: None,
            kill_on_close: true,
            wall_time: None,
            cpu_time: None,
        }
    }

    pub fn with_resource_overrides(
        mut self,
        resources: &crate::config::schema::ResourceLimitsConfig,
    ) -> Self {
        if let Some(mb) = resources.max_memory_mb.filter(|v| *v > 0) {
            let bytes = u64::from(mb).saturating_mul(1024 * 1024);
            self.process_memory_bytes = Some(bytes);
            self.job_memory_bytes = Some(bytes.saturating_mul(2));
        }
        if let Some(count) = resources.max_subprocesses.filter(|v| *v > 0) {
            self.max_processes = Some(count);
        }
        if let Some(secs) = resources.max_cpu_time_seconds.filter(|v| *v > 0) {
            self.cpu_time = Some(Duration::from_secs(secs));
        }
        self
    }

    pub fn validated(self) -> std::io::Result<Self> {
        if let Some(rate) = self.cpu_rate_percent {
            if !(1..=100).contains(&rate) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("cpu_rate_percent must be 1..=100, got {rate}"),
                ));
            }
        }
        if let Some(0) = self.max_processes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "max_processes must be >= 1",
            ));
        }
        Ok(self)
    }
}

#[cfg(all(target_os = "windows", feature = "sandbox-windows-job"))]
mod imp {
    use super::JobLimits;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
        JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
        JOB_OBJECT_LIMIT_JOB_TIME, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };

    pub struct JobObjectGuard {
        handle: HANDLE,
    }

    #[allow(unsafe_code)]
    unsafe impl Send for JobObjectGuard {}
    #[allow(unsafe_code)]
    unsafe impl Sync for JobObjectGuard {}

    impl JobObjectGuard {

        pub fn create(limits: JobLimits) -> std::io::Result<Self> {
            let limits = limits.validated()?;

            #[allow(unsafe_code)]
            let handle = unsafe {
                CreateJobObjectW(None, windows::core::PCWSTR::null())
                    .map_err(|e| std::io::Error::other(format!("CreateJobObjectW failed: {e}")))?
            };
            let guard = Self { handle };
            guard.apply_extended_limits(&limits)?;
            guard.apply_cpu_rate(&limits)?;
            Ok(guard)
        }

        fn apply_extended_limits(&self, limits: &JobLimits) -> std::io::Result<()> {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION::default(),
                ..Default::default()
            };

            let mut flags = JOB_OBJECT_LIMIT::default();

            if limits.kill_on_close {
                flags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            }
            if let Some(bytes) = limits.process_memory_bytes {
                flags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.ProcessMemoryLimit = bytes as usize;
            }
            if let Some(bytes) = limits.job_memory_bytes {
                flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                info.JobMemoryLimit = bytes as usize;
            }
            if let Some(count) = limits.max_processes {
                flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
                info.BasicLimitInformation.ActiveProcessLimit = count;
            }
            if let Some(cpu) = limits.cpu_time {
                flags |= JOB_OBJECT_LIMIT_JOB_TIME;
                info.BasicLimitInformation.PerJobUserTimeLimit =
                    (cpu.as_nanos() / 100).min(i64::MAX as u128) as i64;
            }

            info.BasicLimitInformation.LimitFlags = flags;

            #[allow(unsafe_code)]
            unsafe {
                SetInformationJobObject(
                    self.handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .map_err(|e| {
                    std::io::Error::other(format!(
                        "SetInformationJobObject(ExtendedLimitInformation) failed: {e}"
                    ))
                })?;
            }
            Ok(())
        }

        fn apply_cpu_rate(&self, limits: &JobLimits) -> std::io::Result<()> {
            let Some(rate) = limits.cpu_rate_percent else {
                return Ok(());
            };
            let mut info = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
                ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 {
                    CpuRate: rate.saturating_mul(100),
                },
            };

            #[allow(unsafe_code)]
            unsafe {
                SetInformationJobObject(
                    self.handle,
                    JobObjectCpuRateControlInformation,
                    &mut info as *mut _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                )
                .map_err(|e| {
                    std::io::Error::other(format!(
                        "SetInformationJobObject(CpuRateControlInformation) failed: {e}"
                    ))
                })?;
            }
            Ok(())
        }

        pub fn assign(&self, process_handle: HANDLE) -> std::io::Result<()> {
            #[allow(unsafe_code)]
            unsafe {
                AssignProcessToJobObject(self.handle, process_handle).map_err(|e| {
                    std::io::Error::other(format!("AssignProcessToJobObject failed: {e}"))
                })?;
            }
            Ok(())
        }

        pub fn handle(&self) -> HANDLE {
            self.handle
        }
    }

    impl Drop for JobObjectGuard {
        fn drop(&mut self) {
            #[allow(unsafe_code)]
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }

    fn resume_process_threads(pid: u32) -> std::io::Result<()> {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
            THREADENTRY32,
        };
        use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

        #[allow(unsafe_code)]
        unsafe {
            let bad_length =
                windows::Win32::Foundation::ERROR_BAD_LENGTH.to_hresult();
            let mut attempts = 0u32;
            let snapshot = loop {
                match CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
                    Ok(snapshot) => break snapshot,
                    Err(e) if e.code() == bad_length && attempts < 5 => {
                        attempts += 1;
                        std::thread::sleep(std::time::Duration::from_millis(5 * attempts as u64));
                    }
                    Err(e) => {
                        return Err(std::io::Error::other(format!(
                            "CreateToolhelp32Snapshot failed: {e}"
                        )));
                    }
                }
            };
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut resumed = 0usize;
            let mut has_entry = Thread32First(snapshot, &mut entry).is_ok();
            while has_entry {
                if entry.th32OwnerProcessID == pid {
                    match OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) {
                        Ok(thread) => {
                            let result = ResumeThread(thread);
                            let _ = CloseHandle(thread);
                            if result == u32::MAX {
                                let _ = CloseHandle(snapshot);
                                return Err(std::io::Error::other(format!(
                                    "ResumeThread failed for thread {} of process {pid}",
                                    entry.th32ThreadID
                                )));
                            }
                            resumed += 1;
                        }
                        Err(e) => {
                            let _ = CloseHandle(snapshot);
                            return Err(std::io::Error::other(format!(
                                "OpenThread failed for thread {} of process {pid}: {e}",
                                entry.th32ThreadID
                            )));
                        }
                    }
                }
                has_entry = Thread32Next(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
            if resumed == 0 {
                return Err(std::io::Error::other(format!(
                    "no suspended threads found to resume for process {pid}"
                )));
            }
            Ok(())
        }
    }

    pub async fn spawn_in_job(
        mut cmd: tokio::process::Command,
        limits: JobLimits,
    ) -> std::io::Result<(JobObjectGuard, tokio::process::Child)> {
        use windows::Win32::System::Threading::CREATE_SUSPENDED;

        let job = JobObjectGuard::create(limits)?;
        cmd.creation_flags(crate::util::CREATE_NO_WINDOW | CREATE_SUSPENDED.0);
        let mut child = cmd.spawn()?;
        let Some(pid) = child.id() else {
            let _ = child.kill().await;
            return Err(std::io::Error::other(
                "child process id unavailable; cannot resume suspended process inside job object",
            ));
        };
        let Some(raw) = child.raw_handle() else {
            let _ = child.kill().await;
            return Err(std::io::Error::other(
                "child process handle unavailable; cannot attach process to job object",
            ));
        };
        let raw_handle_value = raw as usize;
        let attach_result = tokio::task::spawn_blocking(move || {
            let process_handle = HANDLE(raw_handle_value as *mut std::ffi::c_void);
            job.assign(process_handle)?;
            resume_process_threads(pid)?;
            Ok::<JobObjectGuard, std::io::Error>(job)
        })
        .await;
        let job = match attach_result {
            Ok(Ok(job)) => job,
            Ok(Err(err)) => {
                let _ = child.kill().await;
                return Err(err);
            }
            Err(join_err) => {
                let _ = child.kill().await;
                return Err(std::io::Error::other(format!(
                    "job object attach task failed: {join_err}"
                )));
            }
        };
        Ok((job, child))
    }
}

#[cfg(not(all(target_os = "windows", feature = "sandbox-windows-job")))]
mod imp {
    use super::JobLimits;

    #[derive(Debug, Default)]
    pub struct JobObjectGuard;

    impl JobObjectGuard {

        pub fn create(limits: JobLimits) -> std::io::Result<Self> {

            let _ = limits.validated()?;
            Ok(Self)
        }
    }

    pub async fn spawn_in_job(
        mut cmd: tokio::process::Command,
        limits: JobLimits,
    ) -> std::io::Result<(JobObjectGuard, tokio::process::Child)> {
        let _ = limits.validated()?;
        let child = cmd.spawn()?;
        Ok((JobObjectGuard, child))
    }
}

pub use imp::{spawn_in_job, JobObjectGuard};

use crate::security::traits::Sandbox;
use async_trait::async_trait;
use std::process::Command;

#[derive(Debug, Clone, Copy, Default)]
pub struct JobObjectSandbox {
    pub limits: JobLimits,
}

impl JobObjectSandbox {

    pub fn with_limits(limits: JobLimits) -> Self {
        Self { limits }
    }

    pub fn probe() -> std::io::Result<Self> {
        Self::probe_with(JobLimits::default())
    }

    pub fn probe_with(limits: JobLimits) -> std::io::Result<Self> {
        #[cfg(all(target_os = "windows", feature = "sandbox-windows-job"))]
        {

            let _ = JobObjectGuard::create(limits)?;
            Ok(Self { limits })
        }
        #[cfg(not(all(target_os = "windows", feature = "sandbox-windows-job")))]
        {
            let _ = limits;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Windows Job Object sandbox not available on this target",
            ))
        }
    }
}

#[async_trait]
impl Sandbox for JobObjectSandbox {
    fn wrap_command(&self, _cmd: &mut Command) -> std::io::Result<()> {

        Ok(())
    }

    fn is_available(&self) -> bool {
        cfg!(all(target_os = "windows", feature = "sandbox-windows-job"))
    }

    fn name(&self) -> &str {
        "windows-job-object"
    }

    fn description(&self) -> &str {
        "Windows kernel Job Object with memory / CPU / process-count caps"
    }
}

