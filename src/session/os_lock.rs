// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

pub struct OsAdvisoryLock {
    file: File,
    path: PathBuf,
}

impl OsAdvisoryLock {
    pub fn lock_path_for_key(key: &str) -> PathBuf {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(key.as_bytes());
        let name = format!("{}.lock", hex::encode(&digest[..16]));
        std::env::temp_dir()
            .join("senweavercoding")
            .join("locks")
            .join(name)
    }

    pub fn try_acquire_key(key: &str) -> io::Result<Option<Self>> {
        let path = Self::lock_path_for_key(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        if try_lock_exclusive(&file)? {
            Ok(Some(Self { file, path }))
        } else {
            Ok(None)
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OsAdvisoryLock {
    fn drop(&mut self) {
        let _ = unlock(&self.file);
    }
}

#[cfg(unix)]
fn try_lock_exclusive(file: &File) -> io::Result<bool> {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(false),
        _ => Err(err),
    }
}

#[cfg(unix)]
fn unlock(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn try_lock_exclusive(file: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if ok != 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
        Ok(false)
    } else {
        Err(err)
    }
}

#[cfg(windows)]
fn unlock(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        UnlockFileEx(file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut overlapped)
    };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(unix, windows)))]
fn try_lock_exclusive(_file: &File) -> io::Result<bool> {
    Ok(true)
}

#[cfg(not(any(unix, windows)))]
fn unlock(_file: &File) -> io::Result<()> {
    Ok(())
}
