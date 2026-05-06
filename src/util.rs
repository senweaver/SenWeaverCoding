// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Utility functions for `SenWeaverCoding`.
//!
//! This module contains reusable helper functions used across the codebase.

use std::ffi::OsStr;

#[inline]
pub fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {

    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var(key.as_ref(), value.as_ref())
    }
}

#[inline]
pub fn remove_env_var<K: AsRef<OsStr>>(key: K) {

    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var(key.as_ref())
    }
}

const SERIAL_ALLOWED_PATH_PREFIXES: &[&str] = &[
    "/dev/ttyACM",
    "/dev/ttyUSB",
    "/dev/tty.usbmodem",
    "/dev/cu.usbmodem",
    "/dev/tty.usbserial",
    "/dev/cu.usbserial",
    "COM",
];

pub fn is_serial_path_allowed(path: &str) -> bool {
    SERIAL_ALLOWED_PATH_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

pub fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => {
            let truncated = &s[..idx];

            format!("{}...", truncated.trim_end())
        }
        None => s.to_string(),
    }
}

pub fn redact_secret(s: &str) -> String {
    if s.is_empty() {
        return "<empty>".to_string();
    }
    format!("<redacted len={}>", s.len())
}

pub enum MaybeSet<T> {
    Set(T),
    Unset,
    Null,
}
