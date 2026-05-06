// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SUBDIR: &str = "token_saver/tee";

pub fn write_failure_log(
    command: &str,
    raw_stdout: &str,
    raw_stderr: &str,
    data_dir: &Path,
) -> Result<PathBuf> {
    let dir = data_dir.join(SUBDIR);
    fs::create_dir_all(&dir)?;
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let slug = sanitize_slug(command);
    let path = dir.join(format!("{epoch}_{slug}.log"));
    let mut f = fs::File::create(&path)?;
    writeln!(f, "# command: {command}")?;
    writeln!(f, "# captured at: {epoch}")?;
    writeln!(f, "# ── stdout ──")?;
    f.write_all(raw_stdout.as_bytes())?;
    if !raw_stdout.ends_with('\n') {
        writeln!(f)?;
    }
    writeln!(f, "# ── stderr ──")?;
    f.write_all(raw_stderr.as_bytes())?;
    if !raw_stderr.ends_with('\n') {
        writeln!(f)?;
    }
    Ok(path)
}

pub fn sanitize_slug(command: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for tok in command.split_whitespace().take(4) {
        let cleaned: String = tok
            .chars()
            .filter_map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                    Some(c)
                } else {
                    None
                }
            })
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
    if s.len() > 60 {
        s.truncate(60);
        while !s.is_char_boundary(s.len()) {
            s.pop();
        }
    }
    s
}

pub fn read_tee_log(path: &Path, max_bytes: usize) -> Result<String> {
    let meta = fs::metadata(path)?;
    if meta.len() as usize > max_bytes {
        let mut f = fs::File::open(path)?;
        let mut buf = vec![0u8; max_bytes];
        use std::io::Read;
        let _ = f.read(&mut buf)?;
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
