// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tool output compression for token savings.
//!
//! Inspired by RTK's approach of compressing tool outputs before they enter
//! the LLM conversation history. This module applies configurable compression
//! strategies to tool results without losing critical information.
//!
//! Strategies (applied in order):
//! 1. **Truncation** - Hard cap on output length with smart boundary detection
//! 2. **Code filtering** - Strip comments, collapse function bodies (for file reads)
//! 3. **Error focus** - Keep only error/warning lines from verbose output
//! 4. **JSON compaction** - Summarize large JSON structures
//! 5. **Line dedup** - Collapse repeated identical/similar lines
//! 6. **Tee** - Save full output to a recovery file when compressed

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolOutputCompressorConfig {
    #[serde(default)]
    pub enabled: bool,

    /// Maximum characters for any single tool output (0 = unlimited).
    #[serde(default = "default_max_output_chars")]
    pub max_output_chars: usize,

    /// Per-tool overrides for max output chars.
    #[serde(default)]
    pub tool_limits: HashMap<String, usize>,

    /// Strip code comments from file_read results.
    #[serde(default = "default_true")]
    pub strip_comments: bool,

    /// Collapse repeated lines (e.g., "line repeated 42 times").
    #[serde(default = "default_true")]
    pub dedup_lines: bool,

    /// Extract only error/warning lines from verbose outputs (shell, test runners).
    #[serde(default)]
    pub error_focus: bool,

    /// Compact large JSON responses (truncate long strings, summarize arrays).
    #[serde(default = "default_true")]
    pub json_compact: bool,

    /// Save full output to a tee file when compression is applied.
    #[serde(default)]
    pub tee_enabled: bool,

    /// Directory for tee files (default: system temp).
    #[serde(default)]
    pub tee_dir: Option<String>,

    /// Maximum tee files to keep (LRU rotation).
    #[serde(default = "default_tee_max_files")]
    pub tee_max_files: usize,
}

fn default_max_output_chars() -> usize {
    50_000
}
fn default_true() -> bool {
    true
}
fn default_tee_max_files() -> usize {
    20
}

impl Default for ToolOutputCompressorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_output_chars: default_max_output_chars(),
            tool_limits: HashMap::new(),
            strip_comments: true,
            dedup_lines: true,
            error_focus: false,
            json_compact: true,
            tee_enabled: false,
            tee_dir: None,
            tee_max_files: default_tee_max_files(),
        }
    }
}

pub struct ToolOutputCompressor {
    config: ToolOutputCompressorConfig,
    tee_dir: Option<PathBuf>,
}

/// Result of compression with metrics.
pub struct CompressionResult {
    pub output: String,
    pub original_chars: usize,
    pub compressed_chars: usize,
    pub tee_path: Option<PathBuf>,
    pub strategies_applied: Vec<&'static str>,
}

impl CompressionResult {
    pub fn savings_pct(&self) -> f64 {
        if self.original_chars == 0 {
            return 0.0;
        }
        (1.0 - self.compressed_chars as f64 / self.original_chars as f64) * 100.0
    }

    pub fn estimated_tokens_saved(&self) -> usize {
        let chars_saved = self.original_chars.saturating_sub(self.compressed_chars);
        chars_saved / 4
    }
}

impl ToolOutputCompressor {
    pub fn new(config: ToolOutputCompressorConfig) -> Self {
        let tee_dir = config.tee_dir.as_ref().map(PathBuf::from).or_else(|| {
            std::env::temp_dir()
                .join("sen-tee")
                .to_str()
                .map(PathBuf::from)
        });

        Self { config, tee_dir }
    }

    /// Returns true if compression is globally disabled via the `enabled` config flag.
    pub fn is_disabled(&self) -> bool {
        !self.config.enabled
    }

    pub fn compress(&self, tool_name: &str, output: &str) -> CompressionResult {
        if !self.config.enabled || output.is_empty() {
            return CompressionResult {
                output: output.to_string(),
                original_chars: output.len(),
                compressed_chars: output.len(),
                tee_path: None,
                strategies_applied: vec![],
            };
        }

        let original_chars = output.len();
        let max_chars = self.max_chars_for_tool(tool_name);
        let mut result = output.to_string();
        let mut strategies: Vec<&'static str> = Vec::new();

        if self.config.dedup_lines {
            let before = result.len();
            result = dedup_lines(&result);
            if result.len() < before {
                strategies.push("line_dedup");
            }
        }

        if self.config.json_compact && looks_like_json(&result) {
            let before = result.len();
            result = compact_json(&result);
            if result.len() < before {
                strategies.push("json_compact");
            }
        }

        if self.config.strip_comments && is_code_tool(tool_name) {
            let before = result.len();
            result = strip_code_comments(&result);
            if result.len() < before {
                strategies.push("strip_comments");
            }
        }

        if self.config.error_focus && is_verbose_tool(tool_name) && result.len() > max_chars {
            let extracted = extract_errors(&result);
            if !extracted.is_empty() && extracted.len() < result.len() {
                result = extracted;
                strategies.push("error_focus");
            }
        }

        let mut tee_path = None;
        if max_chars > 0 && result.len() > max_chars {
            if self.config.tee_enabled {
                tee_path = self.write_tee(tool_name, output);
            }
            result = smart_truncate(&result, max_chars, tee_path.as_ref());
            strategies.push("truncated");
        }

        let compressed_chars = result.len();

        CompressionResult {
            output: result,
            original_chars,
            compressed_chars,
            tee_path,
            strategies_applied: strategies,
        }
    }

    fn max_chars_for_tool(&self, tool_name: &str) -> usize {
        self.config
            .tool_limits
            .get(tool_name)
            .copied()
            .unwrap_or(self.config.max_output_chars)
    }

    fn write_tee(&self, tool_name: &str, full_output: &str) -> Option<PathBuf> {
        let dir = self.tee_dir.as_ref()?;
        if std::fs::create_dir_all(dir).is_err() {
            return None;
        }

        self.rotate_tee_files(dir);

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{}_{}.txt", tool_name, timestamp);
        let path = dir.join(&filename);

        let max_tee_size = 1_048_576; // 1 MiB
        let content = if full_output.len() > max_tee_size {
            &full_output[..max_tee_size]
        } else {
            full_output
        };

        match std::fs::write(&path, content) {
            Ok(()) => Some(path),
            Err(_) => None,
        }
    }

    fn rotate_tee_files(&self, dir: &std::path::Path) {
        let max = self.config.tee_max_files;
        if max == 0 {
            return;
        }

        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "txt"))
            .collect();

        if entries.len() < max {
            return;
        }

        entries.sort_by_key(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });

        let to_remove = entries.len().saturating_sub(max - 1);
        for entry in entries.into_iter().take(to_remove) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn smart_truncate(text: &str, max_chars: usize, tee_path: Option<&PathBuf>) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    let head_ratio = 0.7;
    let head_len = (max_chars as f64 * head_ratio) as usize;
    let tail_len = max_chars.saturating_sub(head_len).saturating_sub(200);

    let head_end = text[..head_len].rfind('\n').unwrap_or(head_len);

    let tail_start = if tail_len > 0 {
        let start_search = text.len().saturating_sub(tail_len);
        text[start_search..]
            .find('\n')
            .map(|p| start_search + p + 1)
            .unwrap_or(start_search)
    } else {
        text.len()
    };

    let omitted = text.len() - head_end - (text.len() - tail_start);
    let omitted_lines = text[head_end..tail_start].matches('\n').count();
    let omitted_tokens_est = omitted / 4;

    let mut result = text[..head_end].to_string();
    result.push_str(&format!(
        "\n\n... [{} chars / ~{} tokens / {} lines omitted] ...\n\n",
        omitted, omitted_tokens_est, omitted_lines
    ));

    if let Some(path) = tee_path {
        result.push_str(&format!("[full output saved: {}]\n\n", path.display()));
    }

    if tail_start < text.len() {
        result.push_str(&text[tail_start..]);
    }

    result
}

fn dedup_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 5 {
        return text.to_string();
    }

    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let mut count = 1usize;

        while i + count < lines.len() && lines[i + count] == line {
            count += 1;
        }

        if count > 2 {
            result.push(line.to_string());
            result.push(format!("  ... (repeated {} more times)", count - 1));
            i += count;
        } else {
            result.push(line.to_string());
            i += 1;
        }
    }

    result.join("\n")
}

fn compact_json(text: &str) -> String {
    let trimmed = text.trim();

    let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return text.to_string(),
    };

    let compacted = compact_json_value(&parsed, 0);
    serde_json::to_string_pretty(&compacted).unwrap_or_else(|_| text.to_string())
}

fn compact_json_value(value: &serde_json::Value, depth: usize) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if s.len() > 200 {
                serde_json::Value::String(format!(
                    "{}... ({} chars)",
                    s.chars().take(100).collect::<String>(),
                    s.len()
                ))
            } else {
                value.clone()
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.len() > 5 && depth > 0 {
                let first_three: Vec<serde_json::Value> = arr
                    .iter()
                    .take(3)
                    .map(|v| compact_json_value(v, depth + 1))
                    .collect();
                let mut result = first_three;
                result.push(serde_json::Value::String(format!(
                    "... ({} more items)",
                    arr.len() - 3
                )));
                serde_json::Value::Array(result)
            } else {
                serde_json::Value::Array(
                    arr.iter()
                        .map(|v| compact_json_value(v, depth + 1))
                        .collect(),
                )
            }
        }
        serde_json::Value::Object(obj) => {
            let compacted: serde_json::Map<String, serde_json::Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), compact_json_value(v, depth + 1)))
                .collect();
            serde_json::Value::Object(compacted)
        }
        _ => value.clone(),
    }
}

fn strip_code_comments(text: &str) -> String {
    let mut result = Vec::new();
    let mut in_block_comment = false;
    let mut consecutive_empty = 0u32;

    for line in text.lines() {
        let trimmed = line.trim();

        if in_block_comment {
            if trimmed.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }

        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            in_block_comment = true;
            continue;
        }

        if trimmed.starts_with("/*") && trimmed.ends_with("*/") {
            continue;
        }

        // Keep doc comments (/// and //!) but strip regular // comments
        if trimmed.starts_with("//") && !trimmed.starts_with("///") && !trimmed.starts_with("//!") {
            continue;
        }

        // Keep Python docstrings but strip # comments
        if trimmed.starts_with('#') && !trimmed.starts_with("#!") && !trimmed.starts_with("#[") {
            continue;
        }

        if trimmed.is_empty() {
            consecutive_empty += 1;
            if consecutive_empty <= 2 {
                result.push(line.to_string());
            }
        } else {
            consecutive_empty = 0;
            result.push(line.to_string());
        }
    }

    result.join("\n")
}

fn extract_errors(text: &str) -> String {
    let error_patterns = [
        "error",
        "Error",
        "ERROR",
        "warning",
        "Warning",
        "WARN",
        "failed",
        "Failed",
        "FAILED",
        "panic",
        "PANIC",
        "traceback",
        "Traceback",
        "exception",
        "Exception",
        "fatal",
        "Fatal",
        "FATAL",
        "deny",
        "Deny",
    ];

    let lines: Vec<&str> = text.lines().collect();
    let mut selected = Vec::new();
    let mut context_after = 0u32;

    for (i, line) in lines.iter().enumerate() {
        let is_error = error_patterns.iter().any(|p| line.contains(p));

        if is_error {
            if i > 0 && (selected.is_empty() || selected.last() != Some(&(i - 1))) {
                selected.push(i - 1);
            }
            selected.push(i);
            context_after = 3;
        } else if context_after > 0 {
            selected.push(i);
            context_after -= 1;
        }
    }

    if selected.is_empty() {
        return String::new();
    }

    selected.sort_unstable();
    selected.dedup();

    let total_lines = lines.len();
    let mut result = Vec::new();
    let mut last_idx: Option<usize> = None;

    for &idx in &selected {
        if idx >= total_lines {
            continue;
        }
        if let Some(last) = last_idx {
            if idx > last + 1 {
                result.push(format!("  ... ({} lines skipped)", idx - last - 1));
            }
        }
        result.push(lines[idx].to_string());
        last_idx = Some(idx);
    }

    let selected_count = selected.len();
    result.push(format!(
        "\n[Error-focused extraction: {} of {} lines shown]",
        selected_count, total_lines
    ));

    result.join("\n")
}

fn is_code_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "file_read" | "content_search" | "glob_search" | "read_skill"
    )
}

fn is_verbose_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "shell" | "delegate" | "llm_task" | "swarm" | "claude_code" | "codex_cli"
    )
}

fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_config() -> ToolOutputCompressorConfig {
        ToolOutputCompressorConfig {
            enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn disabled_passthrough() {
        let compressor = ToolOutputCompressor::new(Default::default());
        let result = compressor.compress("shell", "hello world");
        assert_eq!(result.output, "hello world");
        assert!(result.strategies_applied.is_empty());
        assert_eq!(result.savings_pct(), 0.0);
    }

    #[test]
    fn dedup_repeated_lines() {
        let input = "line1\nrepeated\nrepeated\nrepeated\nrepeated\nrepeated\nline2";
        let output = dedup_lines(input);
        assert!(output.contains("repeated"));
        assert!(output.contains("repeated 4 more times"));
        assert!(output.len() < input.len());
    }

    #[test]
    fn dedup_no_repeats() {
        let input = "line1\nline2\nline3\nline4\nline5";
        let output = dedup_lines(input);
        assert_eq!(output, input);
    }

    #[test]
    fn json_compaction_long_strings() {
        let input = serde_json::json!({
            "key": "A".repeat(300),
            "short": "ok"
        });
        let text = serde_json::to_string_pretty(&input).unwrap();
        let output = compact_json(&text);
        assert!(output.len() < text.len());
        assert!(output.contains("300 chars"));
    }

    #[test]
    fn json_compaction_large_arrays() {
        let arr: Vec<i32> = (0..20).collect();
        let input = serde_json::json!({"data": arr});
        let text = serde_json::to_string_pretty(&input).unwrap();
        let output = compact_json(&text);
        assert!(output.contains("more items"));
    }

    #[test]
    fn json_compaction_invalid_json() {
        let input = "not json at all";
        let output = compact_json(input);
        assert_eq!(output, input);
    }

    #[test]
    fn strip_comments_rust() {
        let input = "// This is a comment\n\
                      /// Doc comment\n\
                      fn main() {\n\
                      // another comment\n\
                      println!(\"hello\");\n\
                      }";
        let output = strip_code_comments(input);
        assert!(!output.contains("This is a comment"));
        assert!(!output.contains("another comment"));
        assert!(output.contains("/// Doc comment"));
        assert!(output.contains("fn main()"));
    }

    #[test]
    fn strip_comments_python() {
        let input = "# comment\n\
                      #! shebang\n\
                      def foo():\n\
                      # inner comment\n\
                      pass";
        let output = strip_code_comments(input);
        assert!(!output.contains("# comment"));
        assert!(!output.contains("# inner comment"));
        assert!(output.contains("#! shebang"));
        assert!(output.contains("def foo():"));
    }

    #[test]
    fn error_extraction() {
        let input = "line 1\nline 2\nerror: something failed\ndetail 1\ndetail 2\n\
                      detail 3\nline 7\nline 8\nwarning: something\nline 10";
        let output = extract_errors(input);
        assert!(output.contains("error: something failed"));
        assert!(output.contains("warning: something"));
        assert!(output.contains("Error-focused extraction"));
    }

    #[test]
    fn smart_truncate_short() {
        let input = "short text";
        let output = smart_truncate(input, 100, None);
        assert_eq!(output, input);
    }

    #[test]
    fn smart_truncate_long() {
        let input = (0..100)
            .map(|i| format!("line {i}: some content here"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = smart_truncate(&input, 500, None);
        assert!(output.len() < input.len());
        assert!(output.contains("omitted"));
    }

    #[test]
    fn full_pipeline() {
        let mut config = enabled_config();
        config.max_output_chars = 200;
        let compressor = ToolOutputCompressor::new(config);

        let input = (0..50)
            .map(|i| format!("line {i}: data"))
            .collect::<Vec<_>>()
            .join("\n");

        let result = compressor.compress("shell", &input);
        assert!(result.compressed_chars < result.original_chars);
        assert!(result.savings_pct() > 0.0);
        assert!(result.estimated_tokens_saved() > 0);
    }

    #[test]
    fn tool_specific_limits() {
        let mut config = enabled_config();
        config.max_output_chars = 10000;
        config.tool_limits.insert("shell".into(), 100);
        let compressor = ToolOutputCompressor::new(config);

        let input = "x".repeat(500);
        let result = compressor.compress("shell", &input);
        assert!(result.compressed_chars < 500);
        assert!(result.strategies_applied.contains(&"truncated"));
    }

    #[test]
    fn savings_metrics() {
        let result = CompressionResult {
            output: String::new(),
            original_chars: 1000,
            compressed_chars: 400,
            tee_path: None,
            strategies_applied: vec![],
        };
        assert!((result.savings_pct() - 60.0).abs() < 0.01);
        assert_eq!(result.estimated_tokens_saved(), 150);
    }
}
