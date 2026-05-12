// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use base64::Engine as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::traits::{Tool, ToolResult};

pub struct DebugTestReportTool;

impl DebugTestReportTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DebugTestReportTool {
    fn default() -> Self {
        Self::new()
    }
}

fn workspace_anchor() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn run_dir(run_id: &str) -> PathBuf {
    workspace_anchor()
        .join(".senagentos")
        .join("debug-reports")
        .join(sanitize_segment(run_id))
}

fn sanitize_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-' | '.' => out.push(ch),
            _ => out.push('_'),
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

fn redact(value: &Value) -> Value {
    crate::services::credential_vault::redact_args_optional(value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum ReportEvent {
    Start {
        run_id: String,
        title: String,
        slug: Option<String>,
        target_urls: Vec<String>,
        started_at: String,
    },
    AddCase {
        run_id: String,
        case_id: String,
        title: String,
        status: String,
        steps: Vec<String>,
        assertions: Vec<Value>,
        screenshots: Vec<String>,
        recorded_at: String,
    },
    AddFinding {
        run_id: String,
        finding_id: String,
        severity: String,
        title: String,
        description: String,
        repro_steps: Vec<String>,
        root_cause: Option<String>,
        fix_suggestion: Option<String>,
        recorded_at: String,
        #[serde(default)]
        category: Option<String>,
        #[serde(default)]
        evidence: Option<Value>,
    },
    AddCoverageEntry {
        run_id: String,
        entry_id: String,
        url: String,
        title: Option<String>,
        depth: i64,
        parent_url: Option<String>,
        http_status: Option<i64>,
        console_errors: Option<i64>,
        network_errors: Vec<Value>,
        visited_at: String,
    },
    RecordNetworkError {
        run_id: String,
        url: String,
        status: i64,
        method: Option<String>,
        page_url: Option<String>,
        when: String,
    },
    AttachScreenshot {
        run_id: String,
        attachment_id: String,
        relative_path: String,
        absolute_path: String,
        caption: Option<String>,
        step_ref: Option<String>,
        recorded_at: String,
    },
    AttachConsoleLogs {
        run_id: String,
        entries: Vec<Value>,
        recorded_at: String,
    },
    Finalize {
        run_id: String,
        summary_note: Option<String>,
        finalized_at: String,
        report_path: String,
    },
}

async fn append_event(run_id: &str, event: &ReportEvent) -> anyhow::Result<PathBuf> {
    let dir = run_dir(run_id);
    tokio::fs::create_dir_all(&dir).await?;
    let jsonl = dir.join("run.jsonl");
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&jsonl)
        .await?;
    file.write_all(line.as_bytes()).await?;
    file.flush().await?;
    Ok(jsonl)
}

async fn read_events(run_id: &str) -> anyhow::Result<Vec<ReportEvent>> {
    let jsonl = run_dir(run_id).join("run.jsonl");
    if !jsonl.exists() {
        return Ok(Vec::new());
    }
    let text = tokio::fs::read_to_string(&jsonl).await?;
    let mut events = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let evt: ReportEvent = serde_json::from_str(trimmed)?;
        events.push(evt);
    }
    Ok(events)
}

fn ok(payload: Value) -> ToolResult {
    ToolResult {
        success: true,
        output: serde_json::to_string_pretty(&payload).unwrap_or_default(),
        error: None,
    }
}

fn err(message: impl Into<String>) -> ToolResult {
    ToolResult {
        success: false,
        output: String::new(),
        error: Some(message.into()),
    }
}

fn timestamp_now() -> String {
    Utc::now().to_rfc3339()
}

fn redact_str(s: &str) -> String {
    crate::services::credential_vault::redact_for_audit_optional(s)
}

fn redact_string_vec(items: &[String]) -> Vec<String> {
    items.iter().map(|s| redact_str(s)).collect()
}

fn normalize_md_path(input: &str) -> String {
    input.replace('\\', "/")
}

fn normalize_path_vec(items: &[String]) -> Vec<String> {
    items.iter().map(|s| normalize_md_path(s)).collect()
}

#[async_trait]
impl Tool for DebugTestReportTool {
    fn name(&self) -> &str {
        "debug_test_report"
    }

    fn description(&self) -> &str {
        "Structured QA debugging report tool. Use start/add_case/add_finding/attach_screenshot/attach_console_logs/add_coverage_entry/record_network_error/finalize to build a Markdown bug report with run.jsonl persistence in .senagentos/debug-reports/<run_id>/. add_finding accepts category=functional|ui|console|network|security|performance|access and an evidence object. add_coverage_entry tracks per-page coverage with url/depth/parent_url/http_status/console_errors/network_errors so finalize can render a coverage matrix."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "start",
                        "add_case",
                        "add_finding",
                        "attach_screenshot",
                        "attach_console_logs",
                        "add_coverage_entry",
                        "record_network_error",
                        "finalize"
                    ],
                    "description": "Report action to perform"
                },
                "run_id": {"type": "string"},
                "title": {"type": "string"},
                "slug": {"type": "string"},
                "target_urls": {"type": "array", "items": {"type": "string"}},
                "case_id": {"type": "string"},
                "status": {
                    "type": "string",
                    "enum": ["passed", "failed", "skipped", "blocked"]
                },
                "steps": {"type": "array", "items": {"type": "string"}},
                "assertions": {"type": "array"},
                "screenshots": {"type": "array", "items": {"type": "string"}},
                "severity": {
                    "type": "string",
                    "enum": ["critical", "high", "medium", "low", "info"]
                },
                "category": {
                    "type": "string",
                    "enum": ["functional", "ui", "console", "network", "security", "performance", "access"],
                    "description": "Finding category. Use for add_finding."
                },
                "evidence": {
                    "type": "object",
                    "description": "Optional evidence bundle for a finding (e.g. {url, screenshot, console_logs[], network[]})."
                },
                "description": {"type": "string"},
                "repro_steps": {"type": "array", "items": {"type": "string"}},
                "root_cause": {"type": "string"},
                "fix_suggestion": {"type": "string"},
                "png_base64": {"type": "string"},
                "src_path": {"type": "string"},
                "step_ref": {"type": "string"},
                "caption": {"type": "string"},
                "entries": {"type": "array"},
                "summary_note": {"type": "string"},
                "url": {"type": "string", "description": "Page URL for add_coverage_entry or record_network_error."},
                "depth": {"type": "integer", "description": "BFS depth for add_coverage_entry."},
                "parent_url": {"type": "string"},
                "http_status": {"type": "integer"},
                "console_errors": {"type": "integer"},
                "network_errors": {"type": "array"},
                "visited_at": {"type": "string"},
                "page_url": {"type": "string"},
                "method": {"type": "string"},
                "when": {"type": "string"}
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let action = match args.get("action").and_then(|v| v.as_str()) {
            Some(a) => a.to_string(),
            None => return Ok(err("missing 'action'")),
        };

        match action.as_str() {
            "start" => action_start(&args).await,
            "add_case" => action_add_case(&args).await,
            "add_finding" => action_add_finding(&args).await,
            "attach_screenshot" => action_attach_screenshot(&args).await,
            "attach_console_logs" => action_attach_console_logs(&args).await,
            "add_coverage_entry" => action_add_coverage_entry(&args).await,
            "record_network_error" => action_record_network_error(&args).await,
            "finalize" => action_finalize(&args).await,
            other => Ok(err(format!("unknown action '{other}'"))),
        }
    }
}

async fn action_start(args: &Value) -> anyhow::Result<ToolResult> {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Debug QA Run")
        .to_string();
    let slug = args.get("slug").and_then(|v| v.as_str()).map(String::from);
    let target_urls: Vec<String> = args
        .get("target_urls")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let timestamp = Utc::now();
    let stamp = timestamp.format("%Y%m%d-%H%M%S").to_string();
    let slug_part = slug
        .clone()
        .map(|s| sanitize_segment(&s))
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string()[..8].to_string());
    let run_id = format!("{}-{}", stamp, slug_part);

    let event = ReportEvent::Start {
        run_id: run_id.clone(),
        title: redact_str(&title),
        slug: slug.clone(),
        target_urls: redact_string_vec(&target_urls),
        started_at: timestamp.to_rfc3339(),
    };
    let jsonl = append_event(&run_id, &event).await?;

    Ok(ok(json!({
        "run_id": run_id,
        "title": title,
        "slug": slug,
        "target_urls": target_urls,
        "jsonl": jsonl.to_string_lossy(),
        "report_dir": run_dir(&run_id).to_string_lossy(),
    })))
}

async fn action_add_case(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled case")
        .to_string();
    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("passed")
        .to_string();
    let steps: Vec<String> = args
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let assertions_raw: Vec<Value> = args
        .get("assertions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let screenshots: Vec<String> = args
        .get("screenshots")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let case_id = format!("case-{}", Uuid::new_v4().simple());
    let event = ReportEvent::AddCase {
        run_id: run_id.clone(),
        case_id: case_id.clone(),
        title: redact_str(&title),
        status,
        steps: redact_string_vec(&steps),
        assertions: assertions_raw.iter().map(redact).collect(),
        screenshots: normalize_path_vec(&screenshots),
        recorded_at: timestamp_now(),
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "case_id": case_id,
    })))
}

async fn action_add_finding(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let severity = args
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Finding")
        .to_string();
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let repro: Vec<String> = args
        .get("repro_steps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let root_cause = args
        .get("root_cause")
        .and_then(|v| v.as_str())
        .map(String::from);
    let fix_suggestion = args
        .get("fix_suggestion")
        .and_then(|v| v.as_str())
        .map(String::from);
    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase());
    let evidence = args.get("evidence").cloned().map(|v| redact(&v));
    let finding_id = format!("finding-{}", Uuid::new_v4().simple());
    let event = ReportEvent::AddFinding {
        run_id: run_id.clone(),
        finding_id: finding_id.clone(),
        severity,
        title: redact_str(&title),
        description: redact_str(&description),
        repro_steps: redact_string_vec(&repro),
        root_cause: root_cause.as_deref().map(redact_str),
        fix_suggestion: fix_suggestion.as_deref().map(redact_str),
        recorded_at: timestamp_now(),
        category,
        evidence,
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "finding_id": finding_id,
    })))
}

async fn action_add_coverage_entry(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => return Ok(err("missing 'url'")),
    };
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .map(String::from);
    let depth = args.get("depth").and_then(|v| v.as_i64()).unwrap_or(0);
    let parent_url = args
        .get("parent_url")
        .and_then(|v| v.as_str())
        .map(String::from);
    let http_status = args.get("http_status").and_then(|v| v.as_i64());
    let console_errors = args.get("console_errors").and_then(|v| v.as_i64());
    let network_errors: Vec<Value> = args
        .get("network_errors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|v| redact(&v))
        .collect();
    let visited_at = args
        .get("visited_at")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(timestamp_now);
    let entry_id = format!("cov-{}", Uuid::new_v4().simple());
    let event = ReportEvent::AddCoverageEntry {
        run_id: run_id.clone(),
        entry_id: entry_id.clone(),
        url: redact_str(&url),
        title: title.as_deref().map(redact_str),
        depth,
        parent_url: parent_url.as_deref().map(redact_str),
        http_status,
        console_errors,
        network_errors,
        visited_at,
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "entry_id": entry_id,
        "url": url,
        "depth": depth,
    })))
}

async fn action_record_network_error(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => u.to_string(),
        _ => return Ok(err("missing 'url'")),
    };
    let status = args.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
    let method = args
        .get("method")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let page_url = args
        .get("page_url")
        .and_then(|v| v.as_str())
        .map(String::from);
    let when = args
        .get("when")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(timestamp_now);
    let event = ReportEvent::RecordNetworkError {
        run_id: run_id.clone(),
        url: redact_str(&url),
        status,
        method: method.as_deref().map(redact_str),
        page_url: page_url.as_deref().map(redact_str),
        when,
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "url": url,
        "status": status,
    })))
}

async fn action_attach_screenshot(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let caption = args
        .get("caption")
        .and_then(|v| v.as_str())
        .map(String::from);
    let step_ref = args
        .get("step_ref")
        .and_then(|v| v.as_str())
        .map(String::from);

    let dir = run_dir(&run_id).join("screenshots");
    tokio::fs::create_dir_all(&dir).await?;
    let attachment_id = format!("shot-{}", Uuid::new_v4().simple());

    let (abs_path, relative_path): (PathBuf, String) =
        if let Some(b64) = args.get("png_base64").and_then(|v| v.as_str()) {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .map_err(|e| anyhow::anyhow!("invalid png_base64: {e}"))?;
            let abs = dir.join(format!("{attachment_id}.png"));
            tokio::fs::write(&abs, &bytes).await?;
            let rel = format!(
                ".senagentos/debug-reports/{}/screenshots/{}.png",
                sanitize_segment(&run_id),
                attachment_id
            );
            (abs, rel)
        } else if let Some(src) = args.get("src_path").and_then(|v| v.as_str()) {
            let src_path = PathBuf::from(src);
            if !src_path.exists() {
                return Ok(err(format!("src_path does not exist: {src}")));
            }
            let bytes = tokio::fs::read(&src_path).await?;
            let abs = dir.join(format!("{attachment_id}.png"));
            tokio::fs::write(&abs, &bytes).await?;
            let rel = format!(
                ".senagentos/debug-reports/{}/screenshots/{}.png",
                sanitize_segment(&run_id),
                attachment_id
            );
            (abs, rel)
        } else {
            return Ok(err(
                "attach_screenshot requires either 'png_base64' or 'src_path'",
            ));
        };

    let event = ReportEvent::AttachScreenshot {
        run_id: run_id.clone(),
        attachment_id: attachment_id.clone(),
        relative_path: relative_path.clone(),
        absolute_path: abs_path.to_string_lossy().to_string(),
        caption: caption.as_deref().map(redact_str),
        step_ref,
        recorded_at: timestamp_now(),
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "attachment_id": attachment_id,
        "path": relative_path,
        "saved_to": abs_path.to_string_lossy(),
    })))
}

async fn action_attach_console_logs(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let entries: Vec<Value> = args
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let redacted: Vec<Value> = entries.iter().map(redact).collect();
    let event = ReportEvent::AttachConsoleLogs {
        run_id: run_id.clone(),
        entries: redacted.clone(),
        recorded_at: timestamp_now(),
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "appended": redacted.len(),
    })))
}

async fn action_finalize(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let summary_note = args
        .get("summary_note")
        .and_then(|v| v.as_str())
        .map(String::from);

    let events = read_events(&run_id).await?;
    let report_md = render_report(&events, summary_note.as_deref());
    let report_path = run_dir(&run_id).join("report.md");
    if let Some(parent) = report_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&report_path, &report_md).await?;
    let report_path_str = report_path.to_string_lossy().to_string();
    let event = ReportEvent::Finalize {
        run_id: run_id.clone(),
        summary_note: summary_note.as_deref().map(redact_str),
        finalized_at: timestamp_now(),
        report_path: report_path_str.clone(),
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "report_path": report_path_str,
        "relative_path": normalize_md_path(
            &report_path
                .strip_prefix(workspace_anchor())
                .unwrap_or(&report_path)
                .to_string_lossy(),
        ),
    })))
}

struct FindingRow {
    id: String,
    severity: String,
    title: String,
    description: String,
    repro: Vec<String>,
    root_cause: Option<String>,
    fix_suggestion: Option<String>,
    category: Option<String>,
    evidence: Option<Value>,
}

struct CoverageRow {
    url: String,
    title: Option<String>,
    depth: i64,
    http_status: Option<i64>,
    console_errors: Option<i64>,
    network_errors: Vec<Value>,
    parent_url: Option<String>,
}

fn render_report(events: &[ReportEvent], summary_note: Option<&str>) -> String {
    let mut title = String::from("Debug QA Report");
    let mut started_at = String::new();
    let mut target_urls: Vec<String> = Vec::new();
    let mut cases: Vec<(String, String, String, Vec<String>, Vec<Value>, Vec<String>)> = Vec::new();
    let mut findings: Vec<FindingRow> = Vec::new();
    let mut screenshots: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();
    let mut console_groups: Vec<Vec<Value>> = Vec::new();
    let mut run_id_value = String::new();
    let mut coverage_rows: Vec<CoverageRow> = Vec::new();
    let mut standalone_network_errors: Vec<(String, i64, Option<String>, Option<String>, String)> = Vec::new();

    for event in events {
        match event {
            ReportEvent::Start {
                run_id,
                title: t,
                target_urls: urls,
                started_at: ts,
                ..
            } => {
                run_id_value = run_id.clone();
                title = t.clone();
                target_urls = urls.clone();
                started_at = ts.clone();
            }
            ReportEvent::AddCase {
                case_id,
                title,
                status,
                steps,
                assertions,
                screenshots: shots,
                ..
            } => {
                cases.push((
                    case_id.clone(),
                    title.clone(),
                    status.clone(),
                    steps.clone(),
                    assertions.clone(),
                    shots.clone(),
                ));
            }
            ReportEvent::AddFinding {
                finding_id,
                severity,
                title,
                description,
                repro_steps,
                root_cause,
                fix_suggestion,
                category,
                evidence,
                ..
            } => {
                findings.push(FindingRow {
                    id: finding_id.clone(),
                    severity: severity.clone(),
                    title: title.clone(),
                    description: description.clone(),
                    repro: repro_steps.clone(),
                    root_cause: root_cause.clone(),
                    fix_suggestion: fix_suggestion.clone(),
                    category: category.clone(),
                    evidence: evidence.clone(),
                });
            }
            ReportEvent::AttachScreenshot {
                attachment_id,
                relative_path,
                caption,
                step_ref,
                ..
            } => {
                screenshots.push((
                    attachment_id.clone(),
                    relative_path.clone(),
                    caption.clone(),
                    step_ref.clone(),
                ));
            }
            ReportEvent::AttachConsoleLogs { entries, .. } => {
                console_groups.push(entries.clone());
            }
            ReportEvent::AddCoverageEntry {
                url,
                title,
                depth,
                parent_url,
                http_status,
                console_errors,
                network_errors,
                ..
            } => {
                let row = CoverageRow {
                    url: url.clone(),
                    title: title.clone(),
                    depth: *depth,
                    http_status: *http_status,
                    console_errors: *console_errors,
                    network_errors: network_errors.clone(),
                    parent_url: parent_url.clone(),
                };
                if let Some(slot) =
                    coverage_rows.iter_mut().find(|r| r.url == row.url)
                {
                    *slot = row;
                } else {
                    coverage_rows.push(row);
                }
            }
            ReportEvent::RecordNetworkError {
                url,
                status,
                method,
                page_url,
                when,
                ..
            } => {
                standalone_network_errors.push((
                    url.clone(),
                    *status,
                    method.clone(),
                    page_url.clone(),
                    when.clone(),
                ));
            }
            ReportEvent::Finalize { .. } => {}
        }
    }

    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    out.push_str(&format!("- Run ID: `{}`\n", run_id_value));
    if !started_at.is_empty() {
        out.push_str(&format!("- Started: {}\n", started_at));
    }
    if !target_urls.is_empty() {
        out.push_str("- Targets:\n");
        for url in &target_urls {
            out.push_str(&format!("  - {}\n", url));
        }
    }
    if let Some(note) = summary_note {
        if !note.is_empty() {
            out.push_str(&format!("\n> {}\n", note));
        }
    }
    out.push_str("\n## Summary\n\n");
    out.push_str(&format!("- Cases: {}\n", cases.len()));
    out.push_str(&format!("- Findings: {}\n", findings.len()));
    out.push_str(&format!("- Screenshots: {}\n", screenshots.len()));
    out.push_str(&format!(
        "- Console log groups: {}\n",
        console_groups.len()
    ));

    if !cases.is_empty() {
        out.push_str("\n## Test Cases\n");
        for (case_id, ctitle, status, steps, assertions, shots) in &cases {
            out.push_str(&format!("\n### {} — {} `[{}]`\n", case_id, ctitle, status));
            if !steps.is_empty() {
                out.push_str("\n**Steps**\n\n");
                for (i, s) in steps.iter().enumerate() {
                    out.push_str(&format!("{}. {}\n", i + 1, s));
                }
            }
            if !assertions.is_empty() {
                out.push_str("\n**Assertions**\n\n");
                for a in assertions {
                    out.push_str(&format!("- `{}`\n", a));
                }
            }
            if !shots.is_empty() {
                out.push_str("\n**Screenshots**\n\n");
                for s in shots {
                    out.push_str(&format!("- ![]({})\n", normalize_md_path(s)));
                }
            }
        }
    }

    if !findings.is_empty() {
        out.push_str("\n## Findings\n");
        for f in &findings {
            let cat_suffix = match &f.category {
                Some(c) if !c.is_empty() => format!(" <{}>", c),
                _ => String::new(),
            };
            out.push_str(&format!(
                "\n### {} [{}]{} {}\n",
                f.id, f.severity, cat_suffix, f.title
            ));
            if !f.description.is_empty() {
                out.push_str(&format!("\n{}\n", f.description));
            }
            if !f.repro.is_empty() {
                out.push_str("\n**Reproduction**\n\n");
                for (i, s) in f.repro.iter().enumerate() {
                    out.push_str(&format!("{}. {}\n", i + 1, s));
                }
            }
            if let Some(rc) = &f.root_cause {
                out.push_str(&format!("\n**Root cause:** {}\n", rc));
            }
            if let Some(fix) = &f.fix_suggestion {
                out.push_str(&format!("\n**Fix suggestion:** {}\n", fix));
            }
            if let Some(ev) = &f.evidence {
                out.push_str("\n**Evidence**\n\n```json\n");
                out.push_str(&serde_json::to_string_pretty(ev).unwrap_or_default());
                out.push_str("\n```\n");
            }
        }
    }

    if !screenshots.is_empty() {
        out.push_str("\n## Attachments\n\n");
        for (id, rel, caption, step_ref) in &screenshots {
            let label = match (step_ref, caption) {
                (Some(step), Some(cap)) => format!("{} — {} ({})", id, cap, step),
                (None, Some(cap)) => format!("{} — {}", id, cap),
                (Some(step), None) => format!("{} ({})", id, step),
                (None, None) => id.clone(),
            };
            out.push_str(&format!("- ![{}]({})\n", label, normalize_md_path(rel)));
        }
    }

    if !console_groups.is_empty() {
        out.push_str("\n## Console Logs\n\n");
        for (idx, group) in console_groups.iter().enumerate() {
            out.push_str(&format!("### Group {}\n\n```jsonl\n", idx + 1));
            for entry in group {
                out.push_str(&format!("{}\n", entry));
            }
            out.push_str("```\n\n");
        }
    }

    if !coverage_rows.is_empty() {
        let pages_visited = coverage_rows.len();
        let mut origins: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut depth_sum: f64 = 0.0;
        let mut failed_pages = 0usize;
        for row in &coverage_rows {
            if let Some(origin) = extract_origin(&row.url) {
                origins.insert(origin);
            }
            depth_sum += row.depth as f64;
            let console_bad = row.console_errors.unwrap_or(0) > 0;
            let network_bad = !row.network_errors.is_empty();
            let status_bad = row
                .http_status
                .map(|s| !(200..400).contains(&s))
                .unwrap_or(false);
            if console_bad || network_bad || status_bad {
                failed_pages += 1;
            }
        }
        let avg_depth = if pages_visited > 0 {
            depth_sum / pages_visited as f64
        } else {
            0.0
        };

        out.push_str("\n## 覆盖率\n\n");
        out.push_str("### 测试范围\n\n");
        out.push_str(&format!("- 已访问页面: {}\n", pages_visited));
        out.push_str(&format!("- 同源数量: {}\n", origins.len()));
        if !origins.is_empty() {
            for origin in &origins {
                out.push_str(&format!("  - {}\n", origin));
            }
        }
        out.push_str(&format!("- 平均深度: {:.2}\n", avg_depth));
        out.push_str(&format!("- 失败页面: {}\n", failed_pages));

        out.push_str("\n### 覆盖矩阵\n\n");
        out.push_str("| # | URL | Title | Depth | Status | Console err | Network err |\n");
        out.push_str("|---|-----|-------|-------|--------|-------------|-------------|\n");
        for (idx, row) in coverage_rows.iter().enumerate() {
            let title_cell = row
                .title
                .as_deref()
                .unwrap_or("")
                .replace('|', "\\|")
                .replace('\n', " ");
            let status_cell = row
                .http_status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string());
            let console_cell = row
                .console_errors
                .map(|c| c.to_string())
                .unwrap_or_else(|| "0".to_string());
            let network_cell = row.network_errors.len().to_string();
            let url_cell = row.url.replace('|', "\\|");
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                idx + 1,
                url_cell,
                title_cell,
                row.depth,
                status_cell,
                console_cell,
                network_cell,
            ));
        }

        let any_parent = coverage_rows.iter().any(|r| r.parent_url.is_some());
        if any_parent {
            out.push_str("\n### 来源链\n\n");
            for row in &coverage_rows {
                if let Some(parent) = &row.parent_url {
                    out.push_str(&format!("- {} ← {}\n", row.url, parent));
                }
            }
        }
    }

    if !standalone_network_errors.is_empty() {
        out.push_str("\n## 网络错误\n\n");
        out.push_str("| # | Method | URL | Status | Page | When |\n");
        out.push_str("|---|--------|-----|--------|------|------|\n");
        for (idx, (url, status, method, page_url, when)) in
            standalone_network_errors.iter().enumerate()
        {
            let method_cell = method.as_deref().unwrap_or("-");
            let page_cell = page_url.as_deref().unwrap_or("-").replace('|', "\\|");
            let url_cell = url.replace('|', "\\|");
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                idx + 1,
                method_cell,
                url_cell,
                status,
                page_cell,
                when,
            ));
        }
    }

    out
}

fn extract_origin(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let scheme_end = trimmed.find("://")?;
    let scheme = &trimmed[..scheme_end];
    let rest = &trimmed[scheme_end + 3..];
    let host_end = rest.find(['/', '?', '#'].as_ref()).unwrap_or(rest.len());
    let host = &rest[..host_end];
    if host.is_empty() {
        return None;
    }
    Some(format!("{}://{}", scheme.to_ascii_lowercase(), host))
}

#[allow(dead_code)]
fn _unused_path(_p: &Path) {}
