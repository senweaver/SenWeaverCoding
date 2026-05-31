// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub fn open_nofollow_read<P: AsRef<Path>>(path: P) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW as i32)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        verify_no_symlink(&path)?;
        OpenOptions::new().read(true).open(path)
    }
}

pub fn open_nofollow_write<P: AsRef<Path>>(
    path: P,
    create: bool,
    truncate: bool,
    append: bool,
) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create(create)
            .truncate(truncate)
            .append(append)
            .custom_flags(libc::O_NOFOLLOW as i32)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        verify_no_symlink(&path)?;
        OpenOptions::new()
            .write(true)
            .create(create)
            .truncate(truncate)
            .append(append)
            .open(path)
    }
}

pub fn verify_no_symlink<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("path '{}' is a symbolic link", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
mod libc_shim {
    pub(super) const _SENTINEL: i32 = libc::O_NOFOLLOW as i32;
}
