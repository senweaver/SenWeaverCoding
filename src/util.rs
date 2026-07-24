// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod retry;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
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

pub fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path
}

pub fn is_index_skip_dir(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name,
        "target"
            | "node_modules"
            | "__pycache__"
            | "dist"
            | "build"
            | "vendor"
            | "venv"
            | "env"
            | "coverage"
            | "out"
            | "bin"
            | "obj"
            | "Pods"
            | "DerivedData"
            | "bower_components"
    )
}

pub fn normalize_path_for_containment(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(c) = existing.canonicalize() {
            let mut base = strip_verbatim_prefix(c);
            for comp in tail.iter().rev() {
                base.push(comp);
            }
            return lexically_normalize(&base);
        }
        match existing.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                if !existing.pop() {
                    break;
                }
            }
            None => break,
        }
    }
    lexically_normalize(&strip_verbatim_prefix(path.to_path_buf()))
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn path_is_within(child: &Path, ancestor: &Path) -> bool {
    let c = strip_verbatim_prefix(child.to_path_buf());
    let a = strip_verbatim_prefix(ancestor.to_path_buf());
    #[cfg(windows)]
    {
        let cs = c.to_string_lossy().to_lowercase();
        let asr = a.to_string_lossy().to_lowercase();
        let sep_back = format!("{asr}\\");
        let sep_fwd = format!("{asr}/");
        cs == asr || cs.starts_with(&sep_back) || cs.starts_with(&sep_fwd)
    }
    #[cfg(not(windows))]
    {
        c == a || c.starts_with(&a)
    }
}

pub fn path_relative_to(child: &Path, ancestor: &Path) -> Option<PathBuf> {
    let c = strip_verbatim_prefix(child.to_path_buf());
    let a = strip_verbatim_prefix(ancestor.to_path_buf());
    c.strip_prefix(&a).ok().map(Path::to_path_buf)
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
pub fn ceil_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut idx = index;
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
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

pub trait SafeStrSlice {
    fn byte_prefix(&self, max_bytes: usize) -> &str;

    fn byte_suffix(&self, max_bytes: usize) -> &str;
}

impl SafeStrSlice for str {
    #[inline]
    fn byte_prefix(&self, max_bytes: usize) -> &str {
        truncate_str_bytes(self, max_bytes)
    }

    #[inline]
    fn byte_suffix(&self, max_bytes: usize) -> &str {
        let start = self.len().saturating_sub(max_bytes);
        &self[ceil_char_boundary(self, start)..]
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

pub fn truncate_head_tail(text: &str, cap: usize, head_share_percent: usize) -> Option<String> {
    if text.len() <= cap || cap == 0 {
        return None;
    }
    let head_budget = (cap * head_share_percent.min(100) / 100).min(text.len());
    let tail_budget = cap.saturating_sub(head_budget);
    let mut head_end = head_budget;
    while head_end > 0 && !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len().saturating_sub(tail_budget);
    while tail_start < text.len() && !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    if tail_start <= head_end {
        return None;
    }
    let elided = tail_start - head_end;
    Some(format!(
        "{}\n... [{} bytes elided of {} total; head and tail preserved] ...\n{}",
        &text[..head_end],
        elided,
        text.len(),
        &text[tail_start..]
    ))
}

pub fn decode_subprocess_bytes(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_owned(),
        Err(_) => decode_with_active_codepage(raw),
    }
}

#[cfg(windows)]
fn decode_with_active_codepage(raw: &[u8]) -> String {
    let encoding = active_ansi_encoding();
    let (decoded, _, had_errors) = encoding.decode(raw);
    if had_errors && !std::ptr::eq(encoding, encoding_rs::GBK) {
        let (gbk, _, gbk_errors) = encoding_rs::GBK.decode(raw);
        if !gbk_errors {
            return gbk.into_owned();
        }
    }
    decoded.into_owned()
}

#[cfg(windows)]
fn active_ansi_encoding() -> &'static encoding_rs::Encoding {
    use std::sync::OnceLock;
    static ENCODING: OnceLock<&'static encoding_rs::Encoding> = OnceLock::new();
    ENCODING.get_or_init(|| {
        let acp = unsafe { windows_sys::Win32::Globalization::GetACP() };
        match acp {
            936 => encoding_rs::GBK,
            950 => encoding_rs::BIG5,
            932 => encoding_rs::SHIFT_JIS,
            949 => encoding_rs::EUC_KR,
            1250 => encoding_rs::WINDOWS_1250,
            1251 => encoding_rs::WINDOWS_1251,
            1253 => encoding_rs::WINDOWS_1253,
            1254 => encoding_rs::WINDOWS_1254,
            65001 => encoding_rs::UTF_8,
            _ => encoding_rs::WINDOWS_1252,
        }
    })
}

#[cfg(not(windows))]
fn decode_with_active_codepage(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}

pub async fn kill_child_process_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let mut cmd = hidden_async_command("taskkill");
            cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            match tokio::time::timeout(std::time::Duration::from_secs(5), cmd.status()).await {
                Ok(Ok(status)) if status.success() => {}
                Ok(Ok(status)) => {
                    tracing::debug!(
                        pid,
                        code = status.code().unwrap_or(-1),
                        "taskkill /T did not terminate the process tree cleanly; \
                         falling back to direct kill"
                    );
                }
                Ok(Err(err)) => {
                    tracing::debug!(pid, error = %err, "taskkill spawn failed; falling back to direct kill");
                }
                Err(_) => {
                    tracing::debug!(pid, "taskkill timed out; falling back to direct kill");
                }
            }
        }
    }
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            let pgid = pid as libc::pid_t;
            unsafe {
                libc::killpg(pgid, libc::SIGTERM);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
    }
    let _ = child.start_kill();
}

pub fn describe_panic(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::path::PathBuf;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{base}.{}.{nanos}.tmp", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => {
            #[cfg(unix)]
            {
                if let Ok(dir_handle) = std::fs::File::open(&dir) {
                    let _ = dir_handle.sync_all();
                }
            }
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub async fn atomic_write_async(
    path: impl AsRef<std::path::Path>,
    bytes: impl Into<Vec<u8>>,
) -> std::io::Result<()> {
    let path = path.as_ref().to_path_buf();
    let bytes = bytes.into();
    tokio::task::spawn_blocking(move || atomic_write(&path, &bytes))
        .await
        .map_err(std::io::Error::other)?
}
