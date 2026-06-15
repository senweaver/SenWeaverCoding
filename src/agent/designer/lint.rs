// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LintFinding {
    pub severity: &'static str,
    pub rule: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LintReport {
    pub findings: Vec<LintFinding>,
    pub p0: usize,
    pub p1: usize,
    pub p2: usize,
}

impl LintReport {
    fn push(&mut self, severity: &'static str, rule: &'static str, message: String, line: Option<usize>) {
        match severity {
            "P0" => self.p0 += 1,
            "P1" => self.p1 += 1,
            _ => self.p2 += 1,
        }
        self.findings.push(LintFinding {
            severity,
            rule,
            message,
            line,
        });
    }

    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

const INDIGO_HEXES: &[&str] = &[
    "#6366f1", "#4f46e5", "#4338ca", "#3730a3", "#8b5cf6", "#7c3aed", "#a855f7",
];

const EMOJI_TELLS: &[&str] = &["✨", "🚀", "🎯", "⚡", "🔥", "💡"];

const FILLER_PHRASES: &[&str] = &[
    "lorem ipsum",
    "placeholder text",
    "sample content",
    "feature one",
    "feature two",
    "feature three",
];

const PLACEHOLDER_CDNS: &[&str] = &[
    "unsplash.com",
    "placehold.co",
    "placekitten.com",
    "picsum.photos",
];

fn floor_boundary(s: &str, mut idx: usize) -> usize {
    if idx > s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn line_of(haystack: &str, byte_idx: usize) -> usize {
    haystack[..byte_idx.min(haystack.len())]
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
        + 1
}

fn find_all(haystack_lower: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = haystack_lower[from..].find(needle) {
        let idx = from + rel;
        out.push(idx);
        from = idx + needle.len().max(1);
    }
    out
}

fn count_var_accent(lower: &str) -> usize {
    find_all(lower, "var(--accent)").len()
}

fn root_block_range(lower: &str) -> Option<(usize, usize)> {
    let start = lower.find(":root")?;
    let brace = lower[start..].find('{')? + start;
    let mut depth = 0usize;
    for (off, ch) in lower[brace..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((start, brace + off + 1));
                }
            }
            _ => {}
        }
    }
    None
}

fn count_raw_hex_outside_root(content: &str, lower: &str) -> usize {
    let root = root_block_range(lower);
    let bytes = content.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            let mut hexlen = 0;
            while j < bytes.len() && (bytes[j] as char).is_ascii_hexdigit() && hexlen < 8 {
                j += 1;
                hexlen += 1;
            }
            if hexlen == 6 || hexlen == 3 || hexlen == 8 {
                let inside_root = root.map(|(s, e)| i >= s && i < e).unwrap_or(false);
                if !inside_root {
                    count += 1;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    count
}

fn looks_like_trust_gradient(segment: &str) -> bool {
    let s = segment.to_ascii_lowercase();
    let hues = [
        "purple", "violet", "indigo", "blue", "cyan", "pink", "fuchsia", "magenta",
    ];
    let hits = hues.iter().filter(|h| s.contains(**h)).count();
    if hits >= 2 {
        return true;
    }
    let indigo_hits = INDIGO_HEXES.iter().filter(|h| s.contains(**h)).count();
    indigo_hits >= 1 && (s.contains("blue") || s.contains("cyan") || s.contains("pink"))
}

pub fn report_from_deck_outcome(
    outcome: &crate::agent::designer::deck::compile::CompileOutcome,
) -> LintReport {
    let mut report = LintReport::default();
    for finding in &outcome.findings {
        report.push(
            finding.severity,
            "deck-compile",
            format!("{}: {}", finding.location, finding.message),
            None,
        );
    }
    for pending in &outcome.pending_slides {
        report.push(
            "P1",
            "deck-pending-slide",
            format!("slides/{pending}.json is listed in deck.json but not written yet."),
            None,
        );
    }
    report
}

const MERMAID_DIAGRAM_KEYWORDS: &[&str] = &[
    "flowchart",
    "graph",
    "sequencediagram",
    "classdiagram",
    "statediagram",
    "statediagram-v2",
    "erdiagram",
    "gantt",
    "pie",
    "journey",
    "timeline",
    "quadrantchart",
    "gitgraph",
    "mindmap",
    "sankey",
    "sankey-beta",
    "xychart",
    "xychart-beta",
    "block",
    "block-beta",
    "architecture",
    "architecture-beta",
    "c4context",
    "requirementdiagram",
    "packet",
    "packet-beta",
    "kanban",
];

fn forbid_code_fence(content: &str, report: &mut LintReport) {
    if content.contains("```") {
        report.push(
            "P0",
            "diagram-fence",
            "File contains a markdown code fence (```) — diagram source files must be the raw \
             source only, with no fences."
                .to_string(),
            None,
        );
    }
}

pub fn lint_mermaid(content: &str) -> LintReport {
    let mut report = LintReport::default();
    forbid_code_fence(content, &mut report);
    let head = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"));
    let Some(head) = head else {
        report.push("P0", "mermaid-empty", "File has no diagram content.".to_string(), None);
        return report;
    };
    let keyword = head
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(':')
        .to_ascii_lowercase();
    if !MERMAID_DIAGRAM_KEYWORDS
        .iter()
        .any(|k| keyword == *k || keyword.starts_with(&format!("{k}-")))
    {
        report.push(
            "P0",
            "mermaid-type",
            format!(
                "First content line must start with a Mermaid diagram keyword (flowchart, \
                 sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram, gantt, pie, journey, \
                 timeline, quadrantChart, mindmap, ...) — found `{head}`."
            ),
            None,
        );
    }
    report
}

pub fn lint_echarts_json(content: &str) -> LintReport {
    let mut report = LintReport::default();
    forbid_code_fence(content, &mut report);
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(option) => {
            if !option.is_object() {
                report.push(
                    "P0",
                    "echarts-shape",
                    "Top level must be a single ECharts option JSON object.".to_string(),
                    None,
                );
                return report;
            }
            if option.get("series").is_none() {
                report.push(
                    "P0",
                    "echarts-series",
                    "Option has no `series` — the chart would render empty.".to_string(),
                    None,
                );
            }
            if option
                .get("title")
                .and_then(|t| t.get("text"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                report.push(
                    "P1",
                    "echarts-title",
                    "Set `title.text` so the chart is self-describing.".to_string(),
                    None,
                );
            }
            if option.get("tooltip").is_none() {
                report.push(
                    "P1",
                    "echarts-tooltip",
                    "Add a `tooltip` for hover inspection.".to_string(),
                    None,
                );
            }
        }
        Err(e) => {
            report.push(
                "P0",
                "echarts-json",
                format!(
                    "Not valid JSON ({e}). The file must be ONE pure JSON option object — no \
                     JavaScript, no functions, no comments, no trailing commas."
                ),
                None,
            );
        }
    }
    report
}

pub fn lint_mindmap_md(content: &str) -> LintReport {
    let mut report = LintReport::default();
    forbid_code_fence(content, &mut report);
    let mut roots = 0usize;
    let mut items = 0usize;
    for line in content.lines() {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("- ") || trimmed_start.starts_with("* ") {
            items += 1;
            let indent = line.len() - trimmed_start.len();
            if indent == 0 {
                roots += 1;
            }
        }
    }
    if items == 0 {
        report.push(
            "P0",
            "mindmap-list",
            "No list items found — a mind map file is one markdown nested unordered list."
                .to_string(),
            None,
        );
        return report;
    }
    if roots == 0 {
        report.push(
            "P0",
            "mindmap-root",
            "No top-level list item — exactly one un-indented `- root` item is required."
                .to_string(),
            None,
        );
    } else if roots > 1 {
        report.push(
            "P0",
            "mindmap-root",
            format!("{roots} top-level items — a mind map has exactly ONE root; nest everything else under it."),
            None,
        );
    }
    report
}

pub fn lint_html(content: &str) -> LintReport {
    let mut report = LintReport::default();
    let lower = content.to_ascii_lowercase();

    for hex in INDIGO_HEXES {
        for idx in find_all(&lower, hex) {
            let before = lower[..idx].rfind(|c: char| !c.is_whitespace());
            let is_var = before
                .map(|b| {
                    let ctx_start = floor_boundary(&lower, b.saturating_sub(16));
                    lower[ctx_start..idx].contains("var(--")
                })
                .unwrap_or(false);
            if is_var {
                continue;
            }
            report.push(
                "P0",
                "indigo-accent",
                format!("Default Tailwind indigo literal `{hex}` — bind `var(--accent)` from the active design system instead."),
                Some(line_of(content, idx)),
            );
        }
    }

    for idx in find_all(&lower, "linear-gradient") {
        let end = floor_boundary(&lower, (idx + 240).min(lower.len()));
        let segment = &lower[idx..end];
        if looks_like_trust_gradient(segment) {
            report.push(
                "P0",
                "trust-gradient",
                "Two-stop purple/blue/cyan/pink \"trust\" gradient — replace with a flat surface and intentional type.".to_string(),
                Some(line_of(content, idx)),
            );
        }
    }

    for emoji in EMOJI_TELLS {
        for idx in find_all(content, emoji) {
            report.push(
                "P0",
                "emoji-icon",
                format!("Emoji `{emoji}` used as iconography — replace with a 1.6-1.8px monoline SVG using currentColor."),
                Some(line_of(content, idx)),
            );
        }
    }

    for phrase in FILLER_PHRASES {
        if let Some(idx) = lower.find(phrase) {
            report.push(
                "P0",
                "filler-copy",
                format!("Filler copy `{phrase}` — write real, product-specific content."),
                Some(line_of(content, idx)),
            );
        }
    }

    {
        let bytes = content.as_bytes();
        let lbytes = lower.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i];
            if c.is_ascii_digit() {
                let mut j = i + 1;
                while j < bytes.len() && ((bytes[j] as char).is_ascii_digit() || bytes[j] == b'.') {
                    j += 1;
                }
                let mut k = j;
                while k < bytes.len() && bytes[k] == b' ' {
                    k += 1;
                }
                let rest = &lower[k..floor_boundary(&lower, (k + 16).min(lbytes.len()))];
                let multiplier = j < bytes.len()
                    && (bytes[j] == b'x' || content[j..].starts_with('\u{00d7}'));
                if (multiplier && rest.starts_with("faster"))
                    || rest.starts_with("faster")
                    || rest.starts_with("more productive")
                {
                    report.push(
                        "P0",
                        "invented-metric",
                        "Invented performance metric — cite a real source or use a labelled placeholder.".to_string(),
                        Some(line_of(content, i)),
                    );
                }
                i = j;
            } else {
                i += 1;
            }
        }
        if let Some(idx) = lower.find("99.9% uptime") {
            report.push(
                "P0",
                "invented-metric",
                "Invented uptime metric `99.9% uptime` — cite a real source or label it as a placeholder.".to_string(),
                Some(line_of(content, idx)),
            );
        }
    }

    for cdn in PLACEHOLDER_CDNS {
        if let Some(idx) = lower.find(cdn) {
            report.push(
                "P1",
                "placeholder-cdn",
                format!("External placeholder image host `{cdn}` breaks in the sandboxed canvas — use a local CSS placeholder or a real generated asset."),
                Some(line_of(content, idx)),
            );
        }
    }

    let raw_hex = count_raw_hex_outside_root(content, &lower);
    if raw_hex > 12 {
        report.push(
            "P1",
            "raw-hex",
            format!("{raw_hex} raw hex colors outside `:root` — design tokens were not honoured; reference `var(--*)` instead."),
            None,
        );
    }

    let accent_uses = count_var_accent(&lower);
    if accent_uses >= 6 {
        report.push(
            "P1",
            "accent-overuse",
            format!("`var(--accent)` referenced {accent_uses} times — cap at ~2 visible accent uses per screen."),
            None,
        );
    }

    {
        let mut section_total = 0usize;
        let mut section_missing = 0usize;
        for idx in find_all(&lower, "<section") {
            section_total += 1;
            let end = lower[idx..].find('>').map(|e| idx + e).unwrap_or(lower.len());
            let tag = &lower[idx..end];
            if !tag.contains("data-od-id") {
                section_missing += 1;
            }
        }
        if section_total > 0 && section_missing > 0 {
            report.push(
                "P2",
                "missing-od-id",
                format!("{section_missing}/{section_total} <section> elements lack a stable `data-od-id` — add one so canvas point-select and targeted edits stay precise."),
                None,
            );
        }
    }

    report
}

pub fn format_report(rel_path: &str, report: &LintReport) -> String {
    let mut out = format!(
        "Design lint — {rel_path}\nP0: {} · P1: {} · P2: {}\n",
        report.p0, report.p1, report.p2
    );
    if report.is_clean() {
        out.push_str("\nNo findings. The artifact passes the anti-AI-slop checks.");
        return out;
    }
    for f in &report.findings {
        let loc = f
            .line
            .map(|l| format!(" (line {l})"))
            .unwrap_or_default();
        out.push_str(&format!("\n[{}] {}{}: {}", f.severity, f.rule, loc, f.message));
    }
    if report.p0 > 0 {
        out.push_str("\n\nFix every P0 finding before declaring this artifact done.");
    }
    out
}
