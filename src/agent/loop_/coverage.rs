// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const COVERAGE_RESULT_MARKER: &str = "[Coverage] Tool '";

const DEFAULT_CONTENT_SEARCH_PAGE: u64 = 50;

pub struct CoverageLedger {
    file_ranges: HashMap<String, Vec<(u64, u64)>>,
    unpaged_files: HashSet<String>,
    file_blobs: HashMap<String, String>,
    search_ranges: HashMap<String, Vec<(u64, u64)>>,
    search_blobs: HashMap<String, String>,
    fetched_urls: HashSet<String>,
    url_blobs: HashMap<String, String>,
    search_queries: HashSet<String>,
    query_blobs: HashMap<String, String>,
    blocked_urls: HashSet<String>,
}

impl Default for CoverageLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl CoverageLedger {
    pub fn new() -> Self {
        Self {
            file_ranges: HashMap::new(),
            unpaged_files: HashSet::new(),
            file_blobs: HashMap::new(),
            search_ranges: HashMap::new(),
            search_blobs: HashMap::new(),
            fetched_urls: HashSet::new(),
            url_blobs: HashMap::new(),
            search_queries: HashSet::new(),
            query_blobs: HashMap::new(),
            blocked_urls: HashSet::new(),
        }
    }

    pub fn reset(&mut self) {
        self.file_ranges.clear();
        self.unpaged_files.clear();
        self.file_blobs.clear();
        self.search_ranges.clear();
        self.search_blobs.clear();
        self.fetched_urls.clear();
        self.url_blobs.clear();
        self.search_queries.clear();
        self.query_blobs.clear();
        self.blocked_urls.clear();
    }

    pub fn skip_reason(&self, tool: &str, args: &Value) -> Option<String> {
        if let Some(url) = extract_url(args) {
            let key = normalize_url(&url);
            if self.blocked_urls.contains(&key) {
                return Some(format!(
                    "{COVERAGE_RESULT_MARKER}{tool}' already failed for URL '{url}' because the host \
                     is local or private. Do NOT retry this URL; it will not succeed."
                ));
            }
        }

        match tool {
            "file_read" => self.skip_file_read(args),
            "content_search" => self.skip_content_search(args),
            "web_fetch" => self.skip_web_fetch(args),
            "web_search" => self.skip_web_search(args),
            _ => None,
        }
    }

    pub fn record_success(
        &mut self,
        tool: &str,
        args: &Value,
        blob_id: Option<&str>,
        output: &str,
    ) -> bool {
        match tool {
            "file_read" => self.record_file_read(args, blob_id, output),
            "content_search" => self.record_content_search(args, blob_id, output),
            "web_fetch" => self.record_web_fetch(args, blob_id),
            "web_search" => self.record_web_search(args, blob_id),
            _ => false,
        }
    }

    pub fn record_failure(&mut self, args: &Value, output: &str) {
        if !output.contains("Blocked local/private host") {
            return;
        }
        if let Some(url) = extract_url(args) {
            self.blocked_urls.insert(normalize_url(&url));
        }
    }

    pub fn invalidate_all_reads(&mut self) {
        self.file_ranges.clear();
        self.unpaged_files.clear();
        self.file_blobs.clear();
        self.search_ranges.clear();
        self.search_blobs.clear();
    }

    pub fn invalidate_after_mutation(&mut self, tool: &str, args: &Value, output: &str) {
        let keys = mutation_path_keys(tool, args, output);
        if keys.is_empty()
            && matches!(
                tool,
                "glob_edit" | "patch_apply" | "diff_apply" | "code_xfile_refactor"
            )
        {
            self.file_ranges.clear();
            self.unpaged_files.clear();
            self.file_blobs.clear();
            self.search_ranges.clear();
            self.search_blobs.clear();
            return;
        }
        if !keys.is_empty() {
            self.search_ranges.clear();
            self.search_blobs.clear();
        }
        for key in keys {
            let canonical = canonical_file_key(&key);
            self.file_ranges.remove(&key);
            self.unpaged_files.remove(&key);
            self.file_blobs.remove(&key);
            if canonical != key {
                self.file_ranges.remove(&canonical);
                self.unpaged_files.remove(&canonical);
                self.file_blobs.remove(&canonical);
            }
        }
    }

    fn skip_file_read(&self, args: &Value) -> Option<String> {
        let path_raw = args.get("path").and_then(Value::as_str)?;
        let path = canonical_file_key(path_raw);
        if path.is_empty() {
            return None;
        }
        let has_offset = has_number_field(args, "offset");
        let has_limit = has_number_field(args, "limit");
        if !has_offset && !has_limit {
            if !self.unpaged_files.contains(&path) {
                return None;
            }
            return Some(file_skip_message(
                &path,
                None,
                self.file_ranges.get(&path).map(Vec::as_slice).unwrap_or(&[]),
                self.file_blobs.get(&path).map(String::as_str),
                true,
            ));
        }
        let start = json_u64(args.get("offset")).unwrap_or(1).max(1);
        let end = match json_u64(args.get("limit")) {
            Some(limit) => start.saturating_add(limit),
            None => u64::MAX,
        };
        let ranges = self.file_ranges.get(&path).map(Vec::as_slice).unwrap_or(&[]);
        if !is_fully_covered(ranges, start, end) {
            return None;
        }
        Some(file_skip_message(
            &path,
            Some((start, end)),
            ranges,
            self.file_blobs.get(&path).map(String::as_str),
            false,
        ))
    }

    fn record_file_read(&mut self, args: &Value, blob_id: Option<&str>, output: &str) -> bool {
        let Some(path_raw) = args.get("path").and_then(Value::as_str) else {
            return false;
        };
        let path = canonical_file_key(path_raw);
        if path.is_empty() {
            return false;
        }
        if let Some(id) = blob_id {
            self.file_blobs.insert(path.clone(), id.to_string());
        }
        if output.contains("[Compacted view") {
            return self.unpaged_files.insert(path);
        }
        let has_offset = has_number_field(args, "offset");
        let has_limit = has_number_field(args, "limit");
        if !has_offset && !has_limit {
            if let Some((start, end)) = parse_file_read_line_range(output) {
                let ranges = self.file_ranges.entry(path.clone()).or_default();
                let grew = !is_fully_covered(ranges, start, end);
                merge_interval(ranges, start, end);
                let unpaged_new = self.unpaged_files.insert(path);
                return grew || unpaged_new;
            }
            return self.unpaged_files.insert(path);
        }
        let (start, end) = parse_file_read_line_range(output).unwrap_or_else(|| {
            let start = json_u64(args.get("offset")).unwrap_or(1).max(1);
            let end = match json_u64(args.get("limit")) {
                Some(limit) => start.saturating_add(limit),
                None => u64::MAX,
            };
            (start, end)
        });
        let ranges = self.file_ranges.entry(path).or_default();
        let grew = !is_fully_covered(ranges, start, end);
        merge_interval(ranges, start, end);
        grew
    }

    fn skip_content_search(&self, args: &Value) -> Option<String> {
        let key = content_search_key(args)?;
        let (start, end) = content_search_page(args);
        let ranges = self
            .search_ranges
            .get(&key)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !is_fully_covered(ranges, start, end) {
            return None;
        }
        let pattern = args
            .get("pattern")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(".")
            .trim();
        Some(format!(
            "{COVERAGE_RESULT_MARKER}content_search' already ran this turn for pattern '{pattern}' \
             in '{path}' covering results {}. Do NOT repeat this page. If earlier output said more \
             matches exist, page with a higher offset into results that are not yet covered. \
             Change the pattern to explore further.{} Do not re-run the same pattern and offset.",
            format_range(start, end),
            blob_hint(self.search_blobs.get(&key).map(String::as_str))
        ))
    }

    fn record_content_search(&mut self, args: &Value, blob_id: Option<&str>, output: &str) -> bool {
        let Some(key) = content_search_key(args) else {
            return false;
        };
        if let Some(id) = blob_id {
            self.search_blobs.insert(key.clone(), id.to_string());
        }
        let (start, page_end) = content_search_page(args);
        let end = if content_search_has_more_pages(output) {
            page_end
        } else {
            u64::MAX
        };
        let ranges = self.search_ranges.entry(key).or_default();
        let grew = !is_fully_covered(ranges, start, end);
        merge_interval(ranges, start, end);
        grew
    }

    fn skip_web_fetch(&self, args: &Value) -> Option<String> {
        let url = extract_url(args)?;
        let key = normalize_url(&url);
        if !self.fetched_urls.contains(&key) {
            return None;
        }
        Some(format!(
            "{COVERAGE_RESULT_MARKER}web_fetch' already ran this turn for URL '{url}'. \
             Do NOT fetch the same URL again.{} Do not retry with the same arguments.",
            blob_hint(self.url_blobs.get(&key).map(String::as_str))
        ))
    }

    fn record_web_fetch(&mut self, args: &Value, blob_id: Option<&str>) -> bool {
        let Some(url) = extract_url(args) else {
            return false;
        };
        let key = normalize_url(&url);
        if let Some(id) = blob_id {
            self.url_blobs.insert(key.clone(), id.to_string());
        }
        self.fetched_urls.insert(key)
    }

    fn skip_web_search(&self, args: &Value) -> Option<String> {
        let query = extract_query(args)?;
        if !self.search_queries.contains(&query) {
            return None;
        }
        Some(format!(
            "{COVERAGE_RESULT_MARKER}web_search' already ran this turn for this query. \
             Do NOT call it again with the same query; rewrite keywords if you need different \
             results.{} Same-query calls are deduplicated.",
            blob_hint(self.query_blobs.get(&query).map(String::as_str))
        ))
    }

    fn record_web_search(&mut self, args: &Value, blob_id: Option<&str>) -> bool {
        let Some(query) = extract_query(args) else {
            return false;
        };
        if let Some(id) = blob_id {
            self.query_blobs.insert(query.clone(), id.to_string());
        }
        self.search_queries.insert(query)
    }
}

fn file_skip_message(
    path: &str,
    requested: Option<(u64, u64)>,
    covered: &[(u64, u64)],
    blob: Option<&str>,
    unpaged: bool,
) -> String {
    let covered_text = format_ranges(covered);
    let request_text = if unpaged {
        "an unpaged read".to_string()
    } else if let Some((start, end)) = requested {
        format!("lines {}", format_range(start, end))
    } else {
        "this range".to_string()
    };
    let covered_clause = if covered_text.is_empty() {
        String::new()
    } else {
        format!(" Already covered this turn: {covered_text}.")
    };
    format!(
        "{COVERAGE_RESULT_MARKER}file_read' requested {request_text} of '{path}', which is already \
         covered this turn.{covered_clause} Do NOT re-read this range. If you need more of the file, \
         page into lines that are not yet covered using a new offset/limit.{} Do not re-run with the \
         same arguments.",
        blob_hint(blob)
    )
}

fn blob_hint(blob_id: Option<&str>) -> String {
    match blob_id {
        Some(id) => format!(
            " If the earlier output was compacted, retrieve it with tool_result_expand (id=\"{id}\")."
        ),
        None => " If the earlier output is still in the conversation, reuse it.".to_string(),
    }
}

fn format_ranges(ranges: &[(u64, u64)]) -> String {
    ranges
        .iter()
        .map(|(start, end)| format_range(*start, *end))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_range(start: u64, end: u64) -> String {
    if end == u64::MAX {
        format!("{start}-EOF")
    } else {
        let last = end.saturating_sub(1).max(start);
        format!("{start}-{last}")
    }
}

fn is_fully_covered(ranges: &[(u64, u64)], start: u64, end: u64) -> bool {
    if start >= end {
        return true;
    }
    let mut cursor = start;
    for &(seg_start, seg_end) in ranges {
        if seg_end <= cursor {
            continue;
        }
        if seg_start > cursor {
            return false;
        }
        cursor = seg_end;
        if cursor >= end {
            return true;
        }
    }
    false
}

fn merge_interval(ranges: &mut Vec<(u64, u64)>, start: u64, end: u64) {
    if start >= end {
        return;
    }
    ranges.push((start, end));
    ranges.sort_unstable_by_key(|&(s, _)| s);
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    for (seg_start, seg_end) in ranges.drain(..) {
        if let Some(last) = merged.last_mut() {
            if seg_start <= last.1 {
                last.1 = last.1.max(seg_end);
                continue;
            }
        }
        merged.push((seg_start, seg_end));
    }
    *ranges = merged;
}

fn content_search_key(args: &Value) -> Option<String> {
    let pattern = args.get("pattern").and_then(Value::as_str)?.trim();
    if pattern.is_empty() {
        return None;
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .trim();
    let glob = args
        .get("include")
        .and_then(Value::as_str)
        .or_else(|| args.get("glob").and_then(Value::as_str))
        .unwrap_or("")
        .trim();
    let output_mode = args
        .get("output_mode")
        .and_then(Value::as_str)
        .unwrap_or("content")
        .trim();
    let case_sensitive = args
        .get("case_sensitive")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let normalized_pattern = if case_sensitive {
        pattern.to_string()
    } else {
        pattern.to_ascii_lowercase()
    };
    Some(format!(
        "{}\n{}\n{}\n{}",
        canonical_file_key(path),
        normalized_pattern,
        glob,
        output_mode
    ))
}

fn content_search_page(args: &Value) -> (u64, u64) {
    let start = json_u64(args.get("offset")).unwrap_or(0);
    let limit = json_u64(args.get("max_results"))
        .unwrap_or(DEFAULT_CONTENT_SEARCH_PAGE)
        .max(1);
    (start, start.saturating_add(limit))
}

fn extract_url(args: &Value) -> Option<String> {
    args.get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn extract_query(args: &Value) -> Option<String> {
    args.get("query")
        .and_then(Value::as_str)
        .map(normalize_query)
        .filter(|s| !s.is_empty())
}

fn normalize_query(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn normalize_path(raw: &str) -> String {
    let replaced = raw.trim().replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in replaced.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
            continue;
        }
        parts.push(part);
    }
    parts.join("/").to_ascii_lowercase()
}

fn content_search_has_more_pages(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    if lower.contains("timed out") {
        return true;
    }
    lower.contains("use offset") || lower.contains("raise max_results")
}

fn parse_file_read_line_range(output: &str) -> Option<(u64, u64)> {
    let marker = "[Lines ";
    let idx = output.rfind(marker)?;
    let rest = &output[idx + marker.len()..];
    let mut nums = rest.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
    let start: u64 = nums.next()?.parse().ok()?;
    let end_incl: u64 = nums.next()?.parse().ok()?;
    if start == 0 || end_incl == 0 {
        return None;
    }
    Some((start, end_incl.saturating_add(1)))
}

fn canonical_file_key(raw: &str) -> String {
    let n = normalize_path(raw);
    if n.is_empty() {
        return n;
    }
    if let Some(ws) = workspace_normalized() {
        let prefix = format!("{ws}/");
        if let Some(rel) = n.strip_prefix(&prefix) {
            if !rel.is_empty() {
                return rel.to_string();
            }
        }
        if n == ws {
            return ".".to_string();
        }
    }
    n
}

fn workspace_normalized() -> Option<String> {
    crate::session::current_session_context()
        .map(|ctx| normalize_path(&ctx.workspace_dir))
        .filter(|s| !s.is_empty())
}

fn coverage_path_keys(raw: &str) -> Vec<String> {
    let n = normalize_path(raw);
    if n.is_empty() {
        return Vec::new();
    }
    let mut keys = vec![n.clone()];
    if let Some(ws) = workspace_normalized() {
        let prefix = format!("{ws}/");
        if let Some(rel) = n.strip_prefix(&prefix) {
            if !rel.is_empty() {
                keys.push(rel.to_string());
            }
        }
        if !n.contains(':') {
            keys.push(format!("{ws}/{n}"));
        }
    }
    keys
}

fn mutation_path_keys(tool: &str, args: &Value, output: &str) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    for key in [
        "path",
        "destination",
        "source",
        "old_path",
        "new_path",
        "notebook_path",
        "target_notebook",
    ] {
        if let Some(value) = args.get(key).and_then(Value::as_str) {
            raw.push(value.to_string());
        }
    }
    if let Some(edits) = args.get("edits").and_then(Value::as_array) {
        for edit in edits {
            if let Some(path) = edit.get("path").and_then(Value::as_str) {
                raw.push(path.to_string());
            }
        }
    }
    if let Some(files) = args.get("files").and_then(Value::as_array) {
        for file in files {
            if let Some(path) = file
                .get("path")
                .and_then(Value::as_str)
                .or_else(|| file.as_str())
            {
                raw.push(path.to_string());
            }
        }
    }
    if let Some(patch) = args.get("patch").and_then(Value::as_str) {
        for line in patch.lines() {
            let candidate = line
                .strip_prefix("+++ ")
                .or_else(|| line.strip_prefix("*** Update File:"))
                .or_else(|| line.strip_prefix("*** Add File:"))
                .or_else(|| line.strip_prefix("*** Delete File:"));
            if let Some(candidate) = candidate {
                let cleaned = candidate
                    .trim()
                    .strip_prefix("b/")
                    .or_else(|| candidate.trim().strip_prefix("a/"))
                    .unwrap_or(candidate.trim());
                if !cleaned.is_empty() && cleaned != "/dev/null" {
                    raw.push(cleaned.to_string());
                }
            }
        }
    }
    if matches!(tool, "glob_edit" | "code_xfile_refactor") {
        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("\u{2713} Edited: ") {
                raw.push(rest.to_string());
            }
        }
    }
    let mut keys: Vec<String> = Vec::new();
    for item in raw {
        for key in coverage_path_keys(&item) {
            let canonical = canonical_file_key(&key);
            if !canonical.is_empty() && !keys.contains(&canonical) {
                keys.push(canonical);
            }
            if !key.is_empty() && !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

fn has_number_field(args: &Value, key: &str) -> bool {
    json_u64(args.get(key)).is_some()
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    match value {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok())),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}
