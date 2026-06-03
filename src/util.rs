// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod retry;

use std::ffi::{OsStr, OsString};
use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use parking_lot::Mutex;

struct ProcessEnvRegistry {
    vars: DashMap<String, String>,
    batch_lock: Mutex<()>,
}

impl ProcessEnvRegistry {
    fn new() -> Self {
        Self {
            vars: DashMap::new(),
            batch_lock: Mutex::new(()),
        }
    }
}

static PROCESS_ENV: OnceLock<Arc<ProcessEnvRegistry>> = OnceLock::new();

fn registry() -> Arc<ProcessEnvRegistry> {
    PROCESS_ENV
        .get_or_init(|| Arc::new(ProcessEnvRegistry::new()))
        .clone()
}

#[inline]
pub fn set_runtime_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    let key = key.as_ref().to_string_lossy().into_owned();
    let value = value.as_ref().to_string_lossy().into_owned();
    registry().vars.insert(key, value);
}

#[inline]
pub fn remove_runtime_var<K: AsRef<OsStr>>(key: K) {
    let key = key.as_ref().to_string_lossy().into_owned();
    registry().vars.remove(&key);
}

pub fn set_runtime_vars_batch(entries: &[(impl AsRef<str>, Option<impl AsRef<str>>)]) {
    let reg = registry();
    let _guard = reg.batch_lock.lock();
    for (key, value) in entries {
        let key = key.as_ref().to_string();
        match value {
            Some(v) => {
                reg.vars
                    .insert(key, v.as_ref().to_string());
            }
            None => {
                reg.vars.remove(&key);
            }
        }
    }
}

pub fn get_runtime_var(key: &str) -> Option<String> {
    if let Some(v) = registry().vars.get(key) {
        return Some(v.clone());
    }
    std::env::var(key).ok()
}

pub fn get_runtime_var_os(key: &str) -> Option<OsString> {
    if let Some(v) = registry().vars.get(key) {
        return Some(OsString::from(v.as_str()));
    }
    std::env::var_os(key)
}

#[inline]
pub fn is_bare_mode() -> bool {
    matches!(
        get_runtime_var("SEN_CLI_BARE").as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
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

#[inline]
pub fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => {
            let truncated = &s[..idx];

            format!("{}...", truncated.trim_end())
        }
        None => s.to_string(),
    }
}

#[inline]
#[must_use]
pub fn floor_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[inline]
#[must_use]
pub fn truncate_str_bytes(s: &str, max_bytes: usize) -> &str {
    &s[..floor_char_boundary(s, max_bytes)]
}

pub fn truncate_string_bytes(s: &mut String, max_bytes: usize) {
    if s.len() > max_bytes {
        let boundary = floor_char_boundary(s, max_bytes);
        s.truncate(boundary);
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

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub trait HiddenWindowCommandExt {
    fn hide_window(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl HiddenWindowCommandExt for std::process::Command {
    fn hide_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }
}

#[cfg(not(windows))]
impl HiddenWindowCommandExt for std::process::Command {
    fn hide_window(&mut self) -> &mut Self {
        self
    }
}

#[cfg(windows)]
impl HiddenWindowCommandExt for tokio::process::Command {
    fn hide_window(&mut self) -> &mut Self {
        self.creation_flags(CREATE_NO_WINDOW)
    }
}

#[cfg(not(windows))]
impl HiddenWindowCommandExt for tokio::process::Command {
    fn hide_window(&mut self) -> &mut Self {
        self
    }
}

pub fn hidden_sync_command<S: AsRef<OsStr>>(program: S) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    cmd.hide_window();
    cmd
}

pub fn hidden_async_command<S: AsRef<OsStr>>(program: S) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.hide_window();
    cmd
}

pub fn decode_subprocess_bytes(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}
