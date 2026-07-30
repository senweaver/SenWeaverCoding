// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use std::borrow::Cow;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SUBDIR: &str = "token_saver/tee";
const STREAM_BYTE_CAP: usize = 8 * 1024 * 1024;
const DIR_TOTAL_BYTE_CAP: u64 = 512 * 1024 * 1024;
const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const CLEANUP_EVERY: u64 = 16;

static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn write_failure_log(
    command: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    data_dir: &Path,
) -> Result<PathBuf> {
    let dir = data_dir.join(SUBDIR);
    fs::create_dir_all(&dir)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let epoch = now.as_secs();
    let epoch_ms = now.as_millis();
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let slug = sanitize_slug(command);
    let path = dir.join(format!("{epoch_ms}_{seq}_{slug}.log"));

    let stdout = cap_stream(raw_stdout);
    let stderr = cap_stream(raw_stderr);
    let mut content =
        String::with_capacity(stdout.len() + stderr.len() + command.len() + 128);
    content.push_str(&format!(
        "# command: {command}\n# captured at: {epoch}\n# ── stdout ──\n"
    ));
    content.push_str(&stdout);
    if !stdout.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("# ── stderr ──\n");
    content.push_str(&stderr);
    if !stderr.ends_with('\n') {
        content.push('\n');
    }

    let run_cleanup = seq % CLEANUP_EVERY == 0;
    let write_path = path.clone();
    let do_write = move || {
        if let Err(err) = fs::write(&write_path, content.as_bytes()) {
            tracing::warn!(
                target: "token_saver.tee",
                error = %err,
                path = %write_path.display(),
                "failed to write tee log"
            );
        }
        if run_cleanup {
            cleanup_dir(&dir);
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn_blocking(do_write);
        }
        Err(_) => do_write(),
    }
    Ok(path)
}

fn cap_stream(text: &str) -> Cow<'_, str> {
    match crate::util::truncate_head_tail(text, STREAM_BYTE_CAP, 50) {
        Some(clipped) => Cow::Owned(clipped),
        None => Cow::Borrowed(text),
    }
}

fn cleanup_dir(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(now);
        let expired = now
            .duration_since(modified)
            .map(|age| age > RETENTION)
            .unwrap_or(false);
        if expired {
            let _ = fs::remove_file(&path);
            continue;
        }
        files.push((modified, meta.len(), path));
    }
    let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if total <= DIR_TOTAL_BYTE_CAP {
        return;
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    for (_, len, path) in files {
        if total <= DIR_TOTAL_BYTE_CAP {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

pub fn sanitize_slug(command: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for tok in command.split_whitespace().take(4) {
        let cleaned: String = tok
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            .collect();
        if !cleaned.is_empty() {
            tokens.push(cleaned);
        }
    }
    let joined = if tokens.is_empty() {
        "cmd".to_string()
    } else {
        tokens.join("-")
    };
    let mut s = joined;
    crate::util::truncate_string_bytes(&mut s, 60);
    s
}

pub fn read_tee_log(path: &Path, max_bytes: usize) -> Result<String> {
    let meta = fs::metadata(path)?;
    if meta.len() as usize > max_bytes {
        let mut f = fs::File::open(path)?;
        let mut buf = vec![0u8; max_bytes];
        let mut filled = 0usize;
        loop {
            let n = f.read(&mut buf[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
            if filled == buf.len() {
                break;
            }
        }
        buf.truncate(filled);
        let mut s = String::from_utf8_lossy(&buf).into_owned();
        s.push_str(&format!(
            "\n... [tee log truncated at {} bytes; full size {} bytes]",
            max_bytes,
            meta.len()
        ));
        return Ok(s);
    }
    Ok(fs::read_to_string(path)?)
}
