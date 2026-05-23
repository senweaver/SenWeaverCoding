// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PREFIX_WINDOW_BYTES: usize = 4096;

pub const SUFFIX_WINDOW_BYTES: usize = 2048;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InlineContext {

    pub imports: Vec<String>,

    pub enclosing_symbol: Option<String>,

    pub recent_files: Vec<PathBuf>,

    pub extra: Vec<String>,
}

impl InlineContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn char_len(&self) -> usize {
        self.imports.iter().map(String::len).sum::<usize>()
            + self.enclosing_symbol.as_deref().unwrap_or("").len()
            + self.extra.iter().map(String::len).sum::<usize>()
    }
}

pub fn build_context_from_window(prefix: &str, _suffix: &str) -> InlineContext {
    let prefix_slice = take_tail(prefix, PREFIX_WINDOW_BYTES);
    let mut imports = Vec::new();
    let mut enclosing: Option<String> = None;

    for raw_line in prefix_slice.lines() {
        let line = raw_line.trim_start();

        if line.starts_with("import ")
            || line.starts_with("from ")
            || line.starts_with("use ")
            || line.starts_with("#include")
            || line.starts_with("require ")
            || line.starts_with("package ")
        {
            imports.push(line.to_string());
            if imports.len() >= 32 {
                break;
            }
        }
    }

    for raw in prefix_slice.lines().rev() {
        let line = raw.trim_start();
        if let Some(rest) = line
            .strip_prefix("fn ")
            .or_else(|| line.strip_prefix("pub fn "))
        {
            enclosing = Some(extract_ident(rest));
            break;
        }
        if let Some(rest) = line.strip_prefix("def ") {
            enclosing = Some(extract_ident(rest));
            break;
        }
        if let Some(rest) = line.strip_prefix("class ") {
            enclosing = Some(extract_ident(rest));
            break;
        }
        if let Some(rest) = line.strip_prefix("function ") {
            enclosing = Some(extract_ident(rest));
            break;
        }
    }

    InlineContext {
        imports,
        enclosing_symbol: enclosing,
        recent_files: Vec::new(),
        extra: Vec::new(),
    }
}

fn take_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    let mut idx = s.len().saturating_sub(max_bytes);
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    &s[idx..]
}

fn extract_ident(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}
