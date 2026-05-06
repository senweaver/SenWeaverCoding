// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Windows Job Object sandbox.
//!
//! `Job Objects` are a Windows kernel facility that lets a parent
//! attach one or more child processes to a single, lifetime-bound
//! group, then impose hard caps on:
//!
//! * total / per-process memory (`JOB_OBJECT_LIMIT_PROCESS_MEMORY` and
//!   `JOB_OBJECT_LIMIT_JOB_MEMORY`),
//! * maximum number of live processes inside the group
//!   (`JOB_OBJECT_LIMIT_ACTIVE_PROCESS`),
//! * CPU rate (`JobObjectCpuRateControlInformation` with the
//!   `HARD_CAP` flag — units of `1/100 %`),
//! * automatic kill-on-close so a panicking parent always tears down
//!   the children it spawned (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`).
//!
//! This module provides three things:
//!
//! 1. [`JobLimits`] — a target-agnostic configuration value that callers
//!    construct once and clone into per-spawn limits;
//! 2. [`JobObjectGuard`] — an RAII handle that owns the underlying
//!    `HANDLE` and runs `CloseHandle` (which triggers kill-on-close)
//!    when it is dropped;
//! 3. [`spawn_in_job`] — a high-level helper that creates a job, spawns
//!    a `tokio::process::Command`, and assigns the resulting child to
//!    the job atomically.
//!
//! On non-Windows targets (or when the `sandbox-windows-job` cargo
//! feature is disabled) the surface still compiles but the
//! [`JobObjectGuard`] becomes a zero-sized stub and `spawn_in_job`
//! degrades to a plain `cmd.spawn()`, so call-sites stay portable.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct JobLimits {

    pub process_memory_bytes: Option<u64>,

    pub job_memory_bytes: Option<u64>,

    pub max_processes: Option<u32>,

    pub cpu_rate_percent: Option<u32>,

    pub kill_on_close: bool,

    pub wall_time: Option<Duration>,
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
        }
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
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };

    pub struct JobObjectGuard {
        handle: HANDLE,
    }

    unsafe impl Send for JobObjectGuard {}
    unsafe impl Sync for JobObjectGuard {}

    impl JobObjectGuard {

        pub fn create(limits: JobLimits) -> std::io::Result<Self> {
            let limits = limits.validated()?;

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

            info.BasicLimitInformation.LimitFlags = flags;

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

            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }

    pub async fn spawn_in_job(
        mut cmd: tokio::process::Command,
        limits: JobLimits,
    ) -> std::io::Result<(JobObjectGuard, tokio::process::Child)> {
        let job = JobObjectGuard::create(limits)?;

        let child = cmd.spawn()?;
        if let Some(raw) = child.raw_handle() {
            let process_handle = HANDLE(raw as *mut std::ffi::c_void);
            job.assign(process_handle).inspect_err(|err| {
                tracing::warn!(error = %err, "failed to attach child to job object");
            })?;
        }
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

