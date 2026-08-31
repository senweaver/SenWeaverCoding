// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::super::traits::{Tool, ToolResult};
use super::engine;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, OnceLock};

const MAX_RESULTS: usize = 1000;
const DEFAULT_MAX_RESULTS: usize = 50;
const MAX_OUTPUT_BYTES: usize = 1_048_576;
const TIMEOUT_SECS: u64 = 30;

pub struct ContentSearchTool {
    security: Arc<SecurityPolicy>,
}

impl ContentSearchTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for ContentSearchTool {
    fn name(&self) -> &str {
        "content_search"
    }

    fn description(&self) -> &str {
        "Search file contents by regex pattern within the workspace using the built-in \
         high-performance search engine (ripgrep core). \
         Output modes: 'content' (matching lines with context), \
         'files_with_matches' (file paths only), 'count' (match counts per file). \
         Example: pattern='fn main', include='*.rs', output_mode='content'."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in, relative to workspace root. Defaults to '.'",
                    "default": "."
                },
                "output_mode": {
                    "type": "string",
                    "description": "Output format: 'content' (matching lines), 'files_with_matches' (paths only), 'count' (match counts)",
                    "enum": ["content", "files_with_matches", "count"],
                    "default": "content"
                },
                "include": {
                    "type": "string",
                    "description": "File glob filter, e.g. '*.rs', '*.{ts,tsx}'"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Case-sensitive matching. Defaults to true",
                    "default": true
                },
                "context_before": {
                    "type": "integer",
                    "description": "Lines of context before each match (content mode only). Default: 3",
                    "default": 3
                },
                "context_after": {
                    "type": "integer",
                    "description": "Lines of context after each match (content mode only). Default: 3",
                    "default": 3
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline matching where patterns can span lines and '.' matches newlines",
                    "default": false
                },
                "encoding": {
                    "type": "string",
                    "description": "Force a specific text encoding for file contents, e.g. 'gbk', 'shift_jis', 'utf-16'. Defaults to automatic (UTF-8 with BOM sniffing)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return per page (content mode counts matching lines, not context lines). Defaults to 50, capped at 1000",
                    "default": 50
                },
                "offset": {
                    "type": "integer",
                    "description": "Number of results to skip for pagination. Defaults to 0",
                    "default": 0
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' parameter"))?;

        if pattern.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Empty pattern is not allowed.".into()),
            });
        }

        let search_path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        let output_mode = args
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("content");

        if !matches!(output_mode, "content" | "files_with_matches" | "count") {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Invalid output_mode '{output_mode}'. Allowed values: content, files_with_matches, count."
                )),
            });
        }

        let include = args.get("include").and_then(|v| v.as_str());

        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        #[allow(clippy::cast_possible_truncation)]
        let context_before = args
            .get("context_before")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        #[allow(clippy::cast_possible_truncation)]
        let context_after = args
            .get("context_after")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        let multiline = args
            .get("multiline")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let encoding = args
            .get("encoding")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        #[allow(clippy::cast_possible_truncation)]
        let page_size = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .min(MAX_RESULTS);

        #[allow(clippy::cast_possible_truncation)]
        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(0);

        let max_results = offset.saturating_add(page_size).min(MAX_RESULTS);

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

        if self.security.is_command_policy_enabled() {
            if std::path::Path::new(search_path).is_absolute()
                && !self.security.is_under_allowed_root(search_path)
            {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Absolute paths are not allowed. Use a relative path.".into()),
                });
            }

            if search_path.contains("../") || search_path.contains("..\\") || search_path == ".." {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Path traversal ('..') is not allowed.".into()),
                });
            }
        }

        if !self.security.is_path_allowed(search_path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Path '{search_path}' is not allowed by security policy."
                )),
            });
        }

        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let resolved_path = self.security.resolve_tool_path(search_path);

        let workspace = self.security.workspace_dir().to_path_buf();
        let canon_outcome = {
            let resolved_path_owned = resolved_path.clone();
            let workspace_owned = workspace.clone();
            tokio::task::spawn_blocking(move || {
                let resolved = std::fs::canonicalize(&resolved_path_owned);
                let ws_canon = std::fs::canonicalize(&workspace_owned)
                    .unwrap_or_else(|_| workspace_owned.clone());
                (resolved, ws_canon)
            })
            .await
            .map_err(|e| anyhow::anyhow!("content_search canonicalize join error: {e}"))?
        };
        let resolved_canon = match canon_outcome.0 {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Cannot resolve path '{search_path}': {e}")),
                });
            }
        };
        let workspace_canon = canon_outcome.1;

        if !self.security.is_resolved_path_allowed(&resolved_canon) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Resolved path for '{search_path}' is outside the allowed workspace."
                )),
            });
        }

        let request = engine::SearchRequest {
            root: resolved_canon,
            pattern: pattern.to_string(),
            fixed_string: false,
            case_sensitive,
            smart_case: false,
            whole_word: false,
            multiline,
            include_globs: include
                .map(|g| vec![g.to_string()])
                .unwrap_or_default(),
            respect_ignore: true,
            include_hidden: false,
            max_file_size: None,
            max_count_per_file: if output_mode == "files_with_matches" {
                Some(1)
            } else {
                None
            },
            context_before: if output_mode == "content" { context_before } else { 0 },
            context_after: if output_mode == "content" { context_after } else { 0 },
            encoding,
            timeout: Some(std::time::Duration::from_secs(TIMEOUT_SECS)),
            max_total_matches: match output_mode {
                "content" | "files_with_matches" => max_results as u64,
                _ => u64::MAX,
            },
            collect_lines: output_mode == "content",
        };

        let output_mode_owned = output_mode.to_string();
        let workspace_canon_owned = workspace_canon.clone();
        let search_outcome = tokio::task::spawn_blocking(move || {
            engine::search(&request).map(|outcome| {
                render_engine_output(
                    &outcome,
                    &workspace_canon_owned,
                    &output_mode_owned,
                    max_results,
                )
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("content_search join error: {e}"))?;

        let formatted = match search_outcome {
            Ok(formatted) => formatted,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Search error: {e}")),
                });
            }
        };

        finalise_search_result(formatted, offset, page_size, output_mode, &workspace_canon)
    }
}

fn relativize(path: &std::path::Path, workspace_canon: &std::path::Path) -> String {
    path.strip_prefix(workspace_canon)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

fn render_engine_output(
    outcome: &engine::SearchOutcome,
    workspace_canon: &std::path::Path,
    output_mode: &str,
    max_results: usize,
) -> String {
    use std::fmt::Write;

    let mut lines: Vec<String> = Vec::new();
    let mut truncated = outcome.truncated;
    let mut file_count: usize = 0;
    let mut total_matches: usize = 0;

    match output_mode {
        "files_with_matches" => {
            for file in &outcome.files {
                if lines.len() >= max_results {
                    truncated = true;
                    break;
                }
                lines.push(relativize(&file.path, workspace_canon));
            }
            file_count = lines.len();
        }
        "count" => {
            for file in &outcome.files {
                if lines.len() >= max_results {
                    truncated = true;
                    break;
                }
                let rel = relativize(&file.path, workspace_canon);
                total_matches += file.match_count as usize;
                lines.push(format!("{rel}:{}", file.match_count));
            }
            file_count = lines.len();
        }
        _ => {
            let with_context = outcome
                .files
                .iter()
                .any(|f| f.lines.iter().any(|l| l.is_context));
            let hard_line_cap = max_results.saturating_mul(12);
            let mut prev_line: Option<u64> = None;
            let mut emitted_any = false;
            'files: for file in &outcome.files {
                let rel = relativize(&file.path, workspace_canon);
                let mut first_in_file = true;
                for lm in &file.lines {
                    if !lm.is_context && total_matches >= max_results {
                        truncated = true;
                        break 'files;
                    }
                    let needs_break = with_context
                        && emitted_any
                        && (first_in_file
                            || prev_line.is_some_and(|prev| lm.line_number > prev + 1));
                    if needs_break {
                        lines.push("--".to_string());
                    }
                    let sep = if lm.is_context { '-' } else { ':' };
                    lines.push(format!("{rel}{sep}{}{sep}{}", lm.line_number, lm.text));
                    if !lm.is_context {
                        total_matches += 1;
                    }
                    prev_line = Some(lm.line_number);
                    first_in_file = false;
                    emitted_any = true;
                    if lines.len() >= hard_line_cap {
                        truncated = true;
                        break 'files;
                    }
                }
                file_count += 1;
            }
        }
    }

    if lines.is_empty() {
        return "No matches found. Note: files that are not valid UTF-8 (e.g. UTF-16) are \
                skipped as binary, and other encodings (e.g. GBK) only match when the \
                `encoding` parameter is specified; if the target may use such an encoding, \
                retry with `encoding` set."
            .to_string();
    }

    let mut buf = lines.join("\n");

    if truncated {
        let _ = write!(
            buf,
            "\n\n[Results truncated: showing first {} matches (max_results={max_results}); raise max_results or use offset to page]",
            if output_mode == "content" { total_matches } else { file_count }
        );
    }
    if outcome.timed_out {
        let _ = write!(
            buf,
            "\n\n[Search timed out after {TIMEOUT_SECS}s: results may be partial]"
        );
    }

    match output_mode {
        "files_with_matches" => {
            let _ = write!(buf, "\n\nTotal: {file_count} files");
        }
        "count" => {
            let _ = write!(
                buf,
                "\n\nTotal: {total_matches} matches in {file_count} files"
            );
        }
        _ => {
            let _ = write!(
                buf,
                "\n\nTotal: {total_matches} matching lines in {file_count} files"
            );
        }
    }

    buf
}

fn split_result_blocks(text: &str) -> Vec<&str> {
    if text.lines().any(|l| l.trim_end() == "--") {
        let mut blocks: Vec<&str> = Vec::new();
        let bytes = text.as_bytes();
        let mut block_start = 0usize;
        let mut line_start = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                let line = &text[line_start..i];
                if line.trim_end() == "--" {
                    if block_start < line_start {
                        blocks.push(text[block_start..line_start].trim_end_matches('\n'));
                    }
                    block_start = i + 1;
                }
                line_start = i + 1;
            }
        }
        if block_start < text.len() {
            let tail = text[block_start..].trim_end_matches('\n');
            if !tail.is_empty() {
                blocks.push(tail);
            }
        }
        blocks
    } else {
        text.lines().filter(|l| !l.is_empty()).collect()
    }
}

fn record_observed_content_paths(
    formatted: &str,
    workspace_canon: &std::path::Path,
    output_mode: &str,
) {
    if output_mode != "content" {
        return;
    }
    let mut seen = std::collections::HashSet::new();
    for line in formatted.lines() {
        if let Some((path, _)) = parse_content_line(line) {
            if seen.insert(path.to_string()) {
                let p = std::path::Path::new(path);
                let abs = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    workspace_canon.join(p)
                };
                crate::session::record_observed_for_current_session(&abs);
            }
        }
    }
}

fn finalise_search_result(
    formatted: String,
    offset: usize,
    max_results: usize,
    output_mode: &str,
    workspace_canon: &std::path::Path,
) -> anyhow::Result<ToolResult> {
    record_observed_content_paths(&formatted, workspace_canon, output_mode);
    let saver_applied = apply_token_saver(&formatted, output_mode);
    let pre_pagination = saver_applied.unwrap_or(formatted);
    let paginated = if offset > 0 {
        let (body, footer) = match pre_pagination.rfind("\n\nTotal: ") {
            Some(idx) => pre_pagination.split_at(idx),
            None => (pre_pagination.as_str(), ""),
        };
        let blocks: Vec<&str> = split_result_blocks(body);
        let total = blocks.len();
        if offset >= total {
            format!("[No more results: offset {offset} >= total {total} results]{footer}")
        } else {
            let end = total.min(offset + max_results);
            let page = &blocks[offset..end];
            let mut out = page.join("\n");
            if total > end {
                out.push_str(&format!(
                    "\n\n[Showing results {}-{} of {total}. Use offset={} for next page]",
                    offset,
                    end,
                    end
                ));
            }
            out.push_str(footer);
            out
        }
    } else {
        pre_pagination
    };

    let final_output = if paginated.len() > MAX_OUTPUT_BYTES {
        let mut truncated = truncate_utf8(&paginated, MAX_OUTPUT_BYTES).to_string();
        truncated.push_str("\n\n[Output truncated: exceeded 1 MB limit]");
        truncated
    } else {
        paginated
    };

    Ok(ToolResult {
        success: true,
        output: final_output,
        error: None,
    })
}

fn apply_token_saver(formatted: &str, output_mode: &str) -> Option<String> {
    if !crate::token_saver::is_enabled() {
        return None;
    }
    let ctx = crate::token_saver::global();
    if matches!(ctx.level, crate::token_saver::CompactLevel::Conservative) {
        return None;
    }
    static MATCH_RE: OnceLock<regex::Regex> = OnceLock::new();
    let match_re = MATCH_RE.get_or_init(|| {
        regex::Regex::new(r"^(?P<path>.*?):(?P<line>\d+):(?P<body>.*)$")
            .expect("saver match regex")
    });
    match output_mode {
        "content" => {
            let mut hits: Vec<crate::token_saver::GrepHit> = Vec::new();
            let mut footer: Vec<String> = Vec::new();
            let mut in_footer = false;
            for line in formatted.lines() {
                if in_footer {
                    footer.push(line.to_string());
                    continue;
                }
                if line.is_empty()
                    || line.starts_with("Total:")
                    || line.starts_with("[Results truncated")
                    || line.starts_with("[Output truncated")
                    || line.starts_with("[Search timed out")
                    || line.starts_with("[Showing lines ")
                    || line == "--"
                {
                    in_footer = true;
                    footer.push(line.to_string());
                    continue;
                }
                if let Some(caps) = match_re.captures(line) {
                    if let (Some(path), Some(line_no), Some(body)) = (
                        caps.name("path").map(|m| m.as_str().to_string()),
                        caps.name("line").and_then(|m| m.as_str().parse::<u64>().ok()),
                        caps.name("body").map(|m| m.as_str().to_string()),
                    ) {
                        hits.push(crate::token_saver::GrepHit {
                            file: path,
                            line_no,
                            line: body,
                        });
                        continue;
                    }
                }

                return None;
            }
            if hits.is_empty() {
                return None;
            }
            let opts = crate::token_saver::GrepOpts {
                level: ctx.level,
                per_file_cap: 5,
                total_cap: 0,
            };
            let mut out = crate::token_saver::compact_grep_results(&hits, &opts);
            for f in footer {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&f);
                out.push('\n');
            }
            Some(out)
        }
        "files_with_matches" => {
            let mut entries: Vec<crate::token_saver::DirEntry> = Vec::new();
            let mut footer: Vec<String> = Vec::new();
            let mut in_footer = false;
            for line in formatted.lines() {
                if in_footer {
                    footer.push(line.to_string());
                    continue;
                }
                if line.is_empty()
                    || line.starts_with("Total:")
                    || line.starts_with("[Results truncated")
                    || line.starts_with("[Output truncated")
                    || line.starts_with("[Search timed out")
                    || line.starts_with("[Showing lines ")
                {
                    in_footer = true;
                    footer.push(line.to_string());
                    continue;
                }
                entries.push(crate::token_saver::DirEntry {
                    name: line.to_string(),
                    is_dir: false,
                    is_hidden: false,
                    size: 0,
                });
            }
            if entries.is_empty() {
                return None;
            }
            let opts = crate::token_saver::ListOpts {
                level: ctx.level,
                group_by_ext: true,
            };
            let mut out = crate::token_saver::compact_dir_listing(&entries, &opts);
            for f in footer {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&f);
                out.push('\n');
            }
            Some(out)
        }

        _ => None,
    }
}

fn parse_content_line(line: &str) -> Option<(&str, bool)> {
    static MATCH_RE: OnceLock<regex::Regex> = OnceLock::new();
    static CONTEXT_RE: OnceLock<regex::Regex> = OnceLock::new();

    let match_re = MATCH_RE.get_or_init(|| {
        regex::Regex::new(r"^(?P<path>.*?):\d+:").expect("match line regex must be valid")
    });
    if let Some(caps) = match_re.captures(line) {
        return caps.name("path").map(|m| (m.as_str(), true));
    }

    let context_re = CONTEXT_RE.get_or_init(|| {
        regex::Regex::new(r"^(?P<path>.*?)-\d+-").expect("context line regex must be valid")
    });
    if let Some(caps) = context_re.captures(line) {
        return caps.name("path").map(|m| (m.as_str(), false));
    }

    None
}

fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
}
