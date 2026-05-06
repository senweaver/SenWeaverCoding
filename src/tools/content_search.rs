// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};

const MAX_RESULTS: usize = 1000;
const MAX_OUTPUT_BYTES: usize = 1_048_576;
const TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Copy)]
enum SearchBackend {
    Ripgrep,
    Grep,
    PureRust,
}

pub struct ContentSearchTool {
    security: Arc<SecurityPolicy>,
    backend: SearchBackend,
}

impl ContentSearchTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        let backend = if which::which("rg").is_ok() {
            SearchBackend::Ripgrep
        } else if which::which("grep").is_ok() {
            SearchBackend::Grep
        } else {
            tracing::info!(
                target: "tools.content_search",
                "neither 'rg' nor 'grep' found on PATH; using pure-Rust fallback"
            );
            SearchBackend::PureRust
        };
        Self { security, backend }
    }

    fn has_rg(&self) -> bool {
        matches!(self.backend, SearchBackend::Ripgrep)
    }
}

#[async_trait]
impl Tool for ContentSearchTool {
    fn name(&self) -> &str {
        "content_search"
    }

    fn description(&self) -> &str {
        "Search file contents by regex pattern within the workspace. \
         Supports ripgrep (rg) with grep fallback. \
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
                    "description": "Enable multiline matching (ripgrep only, errors on grep fallback)",
                    "default": false
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return per page. Defaults to 20",
                    "default": 20
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

        #[allow(clippy::cast_possible_truncation)]
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(MAX_RESULTS)
            .min(MAX_RESULTS);

        #[allow(clippy::cast_possible_truncation)]
        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(0);

        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }

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

        if multiline && !self.has_rg() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Multiline matching requires ripgrep (rg), which is not available.".into(),
                ),
            });
        }

        let formatted = match self.backend {
            SearchBackend::PureRust => {
                let pattern_owned = pattern.to_string();
                let resolved_canon_owned = resolved_canon.clone();
                let include_owned = include.map(|s| s.to_string());
                let output_mode_owned = output_mode.to_string();
                let workspace_canon_owned = workspace_canon.clone();
                let pure_rust_outcome = tokio::task::spawn_blocking(move || {
                    pure_rust_search(
                        &pattern_owned,
                        &resolved_canon_owned,
                        &output_mode_owned,
                        include_owned.as_deref(),
                        case_sensitive,
                        context_before,
                        context_after,
                        max_results,
                        TIMEOUT_SECS,
                    )
                    .map(|raw| {
                        format_line_output(
                            &raw,
                            &workspace_canon_owned,
                            &output_mode_owned,
                            max_results,
                        )
                    })
                })
                .await
                .map_err(|e| anyhow::anyhow!("content_search join error: {e}"))?;

                match pure_rust_outcome {
                    Ok(formatted) => formatted,
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Search error: {e}")),
                        });
                    }
                }
            }
            SearchBackend::Ripgrep | SearchBackend::Grep => {
                let mut cmd = if self.has_rg() {
                    build_rg_command(
                        pattern,
                        &resolved_canon,
                        output_mode,
                        include,
                        case_sensitive,
                        context_before,
                        context_after,
                        multiline,
                    )
                } else {
                    build_grep_command(
                        pattern,
                        &resolved_canon,
                        output_mode,
                        include,
                        case_sensitive,
                        context_before,
                        context_after,
                    )
                };

                cmd.env_clear();
                for key in &["PATH", "HOME", "LANG", "LC_ALL", "LC_CTYPE"] {
                    if let Ok(val) = std::env::var(key) {
                        cmd.env(key, val);
                    }
                }

                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                let output = match tokio::time::timeout(
                    std::time::Duration::from_secs(TIMEOUT_SECS),
                    tokio::process::Command::from(cmd).output(),
                )
                .await
                {
                    Ok(Ok(out)) => out,
                    Ok(Err(e)) => {

                        tracing::warn!(
                            target: "tools.content_search",
                            error = %e,
                            "external search binary failed to launch; falling back to pure-Rust walker"
                        );
                        match pure_rust_search(
                            pattern,
                            &resolved_canon,
                            output_mode,
                            include,
                            case_sensitive,
                            context_before,
                            context_after,
                            max_results,
                            TIMEOUT_SECS,
                        ) {
                            Ok(raw) => {
                                let formatted = format_line_output(
                                    &raw,
                                    &workspace_canon,
                                    output_mode,
                                    max_results,
                                );
                                return finalise_search_result(
                                    formatted,
                                    offset,
                                    max_results,
                                    output_mode,
                                );
                            }
                            Err(fallback_err) => {
                                return Ok(ToolResult {
                                    success: false,
                                    output: String::new(),
                                    error: Some(format!(
                                        "Failed to execute search command: {e} (pure-Rust fallback also failed: {fallback_err})"
                                    )),
                                });
                            }
                        }
                    }
                    Err(_) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Search timed out after {TIMEOUT_SECS} seconds."
                            )),
                        });
                    }
                };

                let exit_code = output.status.code().unwrap_or(-1);
                if exit_code >= 2 {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Search error: {}", stderr.trim())),
                    });
                }

                let raw_stdout = String::from_utf8_lossy(&output.stdout);
                if self.has_rg() {
                    format_rg_output(&raw_stdout, &workspace_canon, output_mode, max_results)
                } else {
                    format_grep_output(
                        &raw_stdout,
                        &workspace_canon,
                        output_mode,
                        max_results,
                    )
                }
            }
        };

        finalise_search_result(formatted, offset, max_results, output_mode)
    }
}

fn finalise_search_result(
    formatted: String,
    offset: usize,
    max_results: usize,
    output_mode: &str,
) -> anyhow::Result<ToolResult> {
    let saver_applied = apply_token_saver(&formatted, output_mode);
    let pre_pagination = saver_applied.unwrap_or(formatted);
    let paginated = if offset > 0 {
        let lines: Vec<&str> = pre_pagination.lines().collect();
        let total = lines.len();
        if offset >= total {
            format!("[No more results: offset {offset} >= total {total} lines]")
        } else {
            let page = &lines[offset..total.min(offset + max_results * 3)];
            let mut out = page.join("\n");
            if total > offset + max_results * 3 {
                out.push_str(&format!(
                    "\n\n[Showing lines {}-{} of {total}. Use offset={} for next page]",
                    offset,
                    offset + page.len(),
                    offset + page.len()
                ));
            }
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
    let match_re = MATCH_RE
        .get_or_init(|| regex::Regex::new(r"^(?P<path>.+?):(?P<line>\d+):(?P<body>.*)$").expect("saver match regex"));
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

fn build_rg_command(
    pattern: &str,
    search_path: &std::path::Path,
    output_mode: &str,
    include: Option<&str>,
    case_sensitive: bool,
    context_before: usize,
    context_after: usize,
    multiline: bool,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("rg");

    cmd.arg("--no-heading");
    cmd.arg("--line-number");
    cmd.arg("--with-filename");

    match output_mode {
        "files_with_matches" => {
            cmd.arg("--files-with-matches");
        }
        "count" => {
            cmd.arg("--count");
        }
        _ => {

            if context_before > 0 {
                cmd.arg("-B").arg(context_before.to_string());
            }
            if context_after > 0 {
                cmd.arg("-A").arg(context_after.to_string());
            }
        }
    }

    if !case_sensitive {
        cmd.arg("-i");
    }

    if multiline {
        cmd.arg("-U");
        cmd.arg("--multiline-dotall");
    }

    if let Some(glob) = include {
        cmd.arg("--glob").arg(glob);
    }

    cmd.arg("--");
    cmd.arg(pattern);
    cmd.arg(search_path);

    cmd
}

fn build_grep_command(
    pattern: &str,
    search_path: &std::path::Path,
    output_mode: &str,
    include: Option<&str>,
    case_sensitive: bool,
    context_before: usize,
    context_after: usize,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("grep");

    cmd.arg("-r");
    cmd.arg("-n");
    cmd.arg("-E");
    cmd.arg("--binary-files=without-match");

    match output_mode {
        "files_with_matches" => {
            cmd.arg("-l");
        }
        "count" => {
            cmd.arg("-c");
        }
        _ => {

            if context_before > 0 {
                cmd.arg("-B").arg(context_before.to_string());
            }
            if context_after > 0 {
                cmd.arg("-A").arg(context_after.to_string());
            }
        }
    }

    if !case_sensitive {
        cmd.arg("-i");
    }

    if let Some(glob) = include {
        cmd.arg("--include").arg(glob);
    }

    cmd.arg("--");
    cmd.arg(pattern);
    cmd.arg(search_path);

    cmd
}

fn format_rg_output(
    raw: &str,
    workspace_canon: &std::path::Path,
    output_mode: &str,
    max_results: usize,
) -> String {
    format_line_output(raw, workspace_canon, output_mode, max_results)
}

fn format_grep_output(
    raw: &str,
    workspace_canon: &std::path::Path,
    output_mode: &str,
    max_results: usize,
) -> String {
    format_line_output(raw, workspace_canon, output_mode, max_results)
}

fn format_line_output(
    raw: &str,
    workspace_canon: &std::path::Path,
    output_mode: &str,
    max_results: usize,
) -> String {
    if raw.trim().is_empty() {
        return "No matches found.".to_string();
    }

    let workspace_prefix = workspace_canon.to_string_lossy();

    let mut lines: Vec<String> = Vec::new();
    let mut truncated = false;
    let mut file_set = std::collections::HashSet::new();
    let mut total_matches: usize = 0;

    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }

        let relativized = relativize_path(line, &workspace_prefix);

        match output_mode {
            "files_with_matches" => {
                let path = relativized.trim();
                if !path.is_empty() && file_set.insert(path.to_string()) {
                    lines.push(path.to_string());
                    if lines.len() >= max_results {
                        truncated = true;
                        break;
                    }
                }
            }
            "count" => {

                if let Some((path, count)) = parse_count_line(&relativized) {
                    if count > 0 {
                        file_set.insert(path.to_string());
                        total_matches += count;
                        lines.push(format!("{path}:{count}"));
                        if lines.len() >= max_results {
                            truncated = true;
                            break;
                        }
                    }
                }
            }
            _ => {

                if relativized == "--" {
                    lines.push(relativized);
                    if lines.len() >= max_results {
                        truncated = true;
                        break;
                    }
                    continue;
                }
                if let Some((path, is_match)) = parse_content_line(&relativized) {
                    file_set.insert(path.to_string());
                    if is_match {
                        total_matches += 1;
                    }
                } else {

                    total_matches += 1;
                }
                lines.push(relativized);
                if lines.len() >= max_results {
                    truncated = true;
                    break;
                }
            }
        }
    }

    if lines.is_empty() {
        return "No matches found.".to_string();
    }

    use std::fmt::Write;
    let mut buf = lines.join("\n");

    if truncated {
        let _ = write!(
            buf,
            "\n\n[Results truncated: showing first {max_results} results]"
        );
    }

    match output_mode {
        "files_with_matches" => {
            let _ = write!(buf, "\n\nTotal: {} files", file_set.len());
        }
        "count" => {
            let _ = write!(
                buf,
                "\n\nTotal: {} matches in {} files",
                total_matches,
                file_set.len()
            );
        }
        _ => {

            let _ = write!(
                buf,
                "\n\nTotal: {} matching lines in {} files",
                total_matches,
                file_set.len()
            );
        }
    }

    buf
}

fn relativize_path(line: &str, workspace_prefix: &str) -> String {
    if let Some(rest) = line.strip_prefix(workspace_prefix) {

        let trimmed = rest
            .strip_prefix('/')
            .or_else(|| rest.strip_prefix('\\'))
            .unwrap_or(rest);
        return trimmed.to_string();
    }
    line.to_string()
}

fn parse_content_line(line: &str) -> Option<(&str, bool)> {
    static MATCH_RE: OnceLock<regex::Regex> = OnceLock::new();
    static CONTEXT_RE: OnceLock<regex::Regex> = OnceLock::new();

    let match_re = MATCH_RE.get_or_init(|| {
        regex::Regex::new(r"^(?P<path>.+?):\d+:").expect("match line regex must be valid")
    });
    if let Some(caps) = match_re.captures(line) {
        return caps.name("path").map(|m| (m.as_str(), true));
    }

    let context_re = CONTEXT_RE.get_or_init(|| {
        regex::Regex::new(r"^(?P<path>.+?)-\d+-").expect("context line regex must be valid")
    });
    if let Some(caps) = context_re.captures(line) {
        return caps.name("path").map(|m| (m.as_str(), false));
    }

    None
}

fn parse_count_line(line: &str) -> Option<(&str, usize)> {
    static COUNT_RE: OnceLock<regex::Regex> = OnceLock::new();
    let count_re = COUNT_RE.get_or_init(|| {
        regex::Regex::new(r"^(?P<path>.+?):(?P<count>\d+)\s*$").expect("count line regex valid")
    });

    let caps = count_re.captures(line)?;
    let path = caps.name("path")?.as_str();
    let count = caps.name("count")?.as_str().parse::<usize>().ok()?;
    Some((path, count))
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

const PURE_RUST_SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    ".tox",
    "__pycache__",
    ".idea",
    ".vscode",
    ".next",
];

#[allow(clippy::too_many_arguments)]
fn pure_rust_search(
    pattern: &str,
    search_root: &std::path::Path,
    output_mode: &str,
    include: Option<&str>,
    case_sensitive: bool,
    context_before: usize,
    context_after: usize,
    max_results: usize,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut regex_builder = regex::RegexBuilder::new(pattern);
    regex_builder.case_insensitive(!case_sensitive);
    let re = regex_builder
        .build()
        .map_err(|e| format!("invalid regex pattern: {e}"))?;

    let include_glob = match include {
        Some(g) if !g.is_empty() => Some(
            globset::GlobBuilder::new(g)
                .literal_separator(false)
                .build()
                .map_err(|e| format!("invalid include glob '{g}': {e}"))?
                .compile_matcher(),
        ),
        _ => None,
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    collect_files(search_root, &include_glob, &deadline, &mut paths)?;

    let mut out = String::new();
    match output_mode {
        "files_with_matches" => {
            let mut emitted = 0usize;
            for p in &paths {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                if file_has_match(p, &re)? {
                    use std::fmt::Write as _;
                    let _ = writeln!(&mut out, "{}", p.display());
                    emitted += 1;
                    if emitted >= max_results {
                        break;
                    }
                }
            }
        }
        "count" => {
            let mut emitted = 0usize;
            for p in &paths {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let count = file_match_count(p, &re)?;
                if count > 0 {
                    use std::fmt::Write as _;
                    let _ = writeln!(&mut out, "{}:{}", p.display(), count);
                    emitted += 1;
                    if emitted >= max_results {
                        break;
                    }
                }
            }
        }
        _ => {

            let mut total_emitted = 0usize;
            'per_file: for p in &paths {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let Ok(raw) = std::fs::read(p) else { continue };
                if looks_binary(&raw) {
                    continue;
                }
                let Ok(text) = std::str::from_utf8(&raw) else {
                    continue;
                };
                let lines: Vec<&str> = text.lines().collect();
                let path_display = p.display().to_string();

                let mut matches: Vec<usize> = Vec::new();
                for (idx, line) in lines.iter().enumerate() {
                    if re.is_match(line) {
                        matches.push(idx);
                    }
                }
                if matches.is_empty() {
                    continue;
                }

                let mut last_emitted: Option<usize> = None;
                for &m in &matches {
                    let start = m.saturating_sub(context_before);
                    let end = (m + context_after).min(lines.len().saturating_sub(1));

                    if let Some(prev) = last_emitted {
                        if start > prev + 1 {
                            out.push_str("--\n");
                        }
                    }

                    for i in start..=end {
                        let line_number = i + 1;
                        let sep = if i == m { ':' } else { '-' };
                        use std::fmt::Write as _;
                        let _ = writeln!(
                            &mut out,
                            "{path_display}{sep}{line_number}{sep}{}",
                            lines[i]
                        );
                        last_emitted = Some(i);
                        total_emitted += 1;
                        if total_emitted >= max_results {
                            break 'per_file;
                        }
                    }
                }
            }
        }
    }

    if std::time::Instant::now() >= deadline {
        return Err(format!(
            "pure-Rust search exceeded {timeout_secs}s before completing"
        ));
    }

    Ok(out)
}

fn collect_files(
    dir: &std::path::Path,
    include: &Option<globset::GlobMatcher>,
    deadline: &std::time::Instant,
    out: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    if std::time::Instant::now() >= *deadline {
        return Ok(());
    }

    if dir.is_file() {
        if glob_accepts(include, dir) {
            out.push(dir.to_path_buf());
        }
        return Ok(());
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        if std::time::Instant::now() >= *deadline {
            return Ok(());
        }
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if file_type.is_dir() {
            if PURE_RUST_SKIP_DIRS.iter().any(|skip| *skip == name) {
                continue;
            }
            collect_files(&path, include, deadline, out)?;
        } else if file_type.is_file() && glob_accepts(include, &path) {
            out.push(path);
        }
    }

    Ok(())
}

fn glob_accepts(include: &Option<globset::GlobMatcher>, path: &std::path::Path) -> bool {
    match include {
        None => true,
        Some(matcher) => {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            matcher.is_match(name) || matcher.is_match(path)
        }
    }
}

fn file_has_match(path: &std::path::Path, re: &regex::Regex) -> Result<bool, String> {
    let Ok(raw) = std::fs::read(path) else {
        return Ok(false);
    };
    if looks_binary(&raw) {
        return Ok(false);
    }
    let Ok(text) = std::str::from_utf8(&raw) else {
        return Ok(false);
    };
    Ok(re.is_match(text))
}

fn file_match_count(path: &std::path::Path, re: &regex::Regex) -> Result<usize, String> {
    let Ok(raw) = std::fs::read(path) else {
        return Ok(0);
    };
    if looks_binary(&raw) {
        return Ok(0);
    }
    let Ok(text) = std::str::from_utf8(&raw) else {
        return Ok(0);
    };
    let count = text.lines().filter(|l| re.is_match(l)).count();
    Ok(count)
}

fn looks_binary(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(8 * 1024)];
    window.contains(&0)
}
