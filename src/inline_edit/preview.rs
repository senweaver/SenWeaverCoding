// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hunk {
    pub header: String,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffPreview {

    pub raw_diff: String,
    pub hunks: Vec<Hunk>,

    pub additions: usize,

    pub deletions: usize,
}

impl DiffPreview {

    pub fn from_unified(diff: &str) -> Self {
        let mut hunks = Vec::new();
        let mut current: Option<Hunk> = None;
        let mut additions = 0usize;
        let mut deletions = 0usize;
        for line in diff.lines() {
            if let Some(header) = line.strip_prefix("@@ ") {
                if let Some(h) = current.take() {
                    hunks.push(h);
                }
                current = Some(Hunk {
                    header: format!("@@ {header}"),
                    old_lines: Vec::new(),
                    new_lines: Vec::new(),
                });
                continue;
            }
            if let Some(h) = current.as_mut() {
                if line.starts_with("---") || line.starts_with("+++") {
                    continue;
                }
                if let Some(body) = line.strip_prefix('+') {
                    h.new_lines.push(body.to_string());
                    additions += 1;
                } else if let Some(body) = line.strip_prefix('-') {
                    h.old_lines.push(body.to_string());
                    deletions += 1;
                } else if let Some(body) = line.strip_prefix(' ') {
                    h.old_lines.push(body.to_string());
                    h.new_lines.push(body.to_string());
                }
            }
        }
        if let Some(h) = current {
            hunks.push(h);
        }
        Self {
            raw_diff: diff.to_string(),
            hunks,
            additions,
            deletions,
        }
    }

    pub fn render_plain(&self) -> String {
        let mut out = String::new();
        for h in &self.hunks {
            out.push_str(&h.header);
            out.push('\n');
            for l in &h.old_lines {
                out.push('-');
                out.push_str(l);
                out.push('\n');
            }
            for l in &h.new_lines {
                out.push('+');
                out.push_str(l);
                out.push('\n');
            }
        }
        out
    }
}
