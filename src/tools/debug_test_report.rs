// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::OnceLock;

use async_trait::async_trait;
use base64::Engine as _;
use chrono::Utc;
use parking_lot::RwLock;
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

static ACTIVE_RUN_ID: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn active_run_slot() -> &'static RwLock<Option<String>> {
    ACTIVE_RUN_ID.get_or_init(|| RwLock::new(None))
}

pub fn set_active_run_id(run_id: Option<String>) {
    *active_run_slot().write() = run_id;
}

pub fn active_run_id() -> Option<String> {
    active_run_slot().read().clone()
}

pub fn record_browser_action(
    action: &str,
    args: &Value,
    success: bool,
) {
    let Some(run_id) = active_run_id() else {
        return;
    };
    let tab_id = args
        .get("tab_id")
        .and_then(|v| v.as_u64())
        .or_else(|| args.get("tab").and_then(|v| v.as_u64()));
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| pii_only_str(s));
    let selector = args
        .get("selector")
        .and_then(|v| v.as_str())
        .map(|s| pii_only_str(s));
    let event = ReportEvent::BrowserAction {
        run_id: run_id.clone(),
        action: action.to_string(),
        tab_id,
        url,
        selector,
        success,
        recorded_at: timestamp_now(),
    };
    tokio::spawn(async move {
        if let Err(err) = append_event(&run_id, &event).await {
            tracing::debug!(target: "debug_test_report.browser_trace", error = %err, "failed to append browser trace");
        }
    });
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
    let after_vault = crate::services::credential_vault::redact_args_optional(value);
    let (after_pii, _) = crate::services::pii_sanitizer::global_sanitizer().sanitize_json(&after_vault);
    after_pii
}

fn pii_only_str(input: &str) -> String {
    let (clean, _) = crate::services::pii_sanitizer::global_sanitizer().sanitize(input);
    clean
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
    BrowserAction {
        run_id: String,
        action: String,
        tab_id: Option<u64>,
        url: Option<String>,
        selector: Option<String>,
        success: bool,
        recorded_at: String,
    },
    Finalize {
        run_id: String,
        summary_note: Option<String>,
        finalized_at: String,
        report_path: String,
    },
    AddTestPlan {
        run_id: String,
        dimensions: Vec<Value>,
        cases_outline: Vec<Value>,
        recorded_at: String,
    },
    AddAnalysisNote {
        run_id: String,
        note_id: String,
        category: String,
        title: String,
        description: String,
        severity: Option<String>,
        evidence_refs: Vec<String>,
        recorded_at: String,
    },
    AddRunbookSection {
        run_id: String,
        section_id: String,
        section_kind: String,
        title: String,
        body: String,
        recorded_at: String,
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
    let after_vault = crate::services::credential_vault::redact_for_audit_optional(s);
    pii_only_str(&after_vault)
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
        "Structured QA debugging report tool. Use start/add_case/add_finding/attach_screenshot/attach_console_logs/add_coverage_entry/record_network_error/add_test_plan/add_analysis_note/add_runbook_section/finalize to build the three QA documents (report.md / analysis.md / runbook.md) with run.jsonl persistence in .senagentos/debug-reports/<run_id>/. add_finding accepts category=functional|ui|console|network|security|performance|access and an evidence object. add_coverage_entry tracks per-page coverage with url/depth/parent_url/http_status/console_errors/network_errors so finalize can render a coverage matrix. add_test_plan submits the QA dimensions+cases outline before execution. add_analysis_note groups findings by category (root_cause|performance|security|a11y|ux|risk). add_runbook_section adds operational steps grouped by section_kind."
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
                        "finalize",
                        "add_test_plan",
                        "add_analysis_note",
                        "add_runbook_section"
                    ],
                    "description": "Report action to perform"
                },
                "dimensions": {
                    "type": "array",
                    "description": "QA test plan dimensions; each item should carry {name, scope, priority}.",
                    "items": {"type": "object"}
                },
                "cases_outline": {
                    "type": "array",
                    "description": "QA test plan case outline; each item should carry {title, dimension, severity}.",
                    "items": {"type": "object"}
                },
                "note_id": {"type": "string"},
                "section_id": {"type": "string"},
                "section_kind": {
                    "type": "string",
                    "enum": [
                        "context",
                        "preconditions",
                        "test_data",
                        "sop_steps",
                        "expected",
                        "regression_checklist",
                        "troubleshooting"
                    ]
                },
                "body": {"type": "string"},
                "evidence_refs": {"type": "array", "items": {"type": "string"}},
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
            "add_test_plan" => action_add_test_plan(&args).await,
            "add_analysis_note" => action_add_analysis_note(&args).await,
            "add_runbook_section" => action_add_runbook_section(&args).await,
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
    set_active_run_id(Some(run_id.clone()));

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

async fn action_add_test_plan(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let dimensions: Vec<Value> = args
        .get("dimensions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|v| redact(&v))
        .collect();
    let cases_outline: Vec<Value> = args
        .get("cases_outline")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|v| redact(&v))
        .collect();
    if dimensions.is_empty() && cases_outline.is_empty() {
        return Ok(err("add_test_plan requires non-empty 'dimensions' or 'cases_outline'"));
    }
    let event = ReportEvent::AddTestPlan {
        run_id: run_id.clone(),
        dimensions: dimensions.clone(),
        cases_outline: cases_outline.clone(),
        recorded_at: timestamp_now(),
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "dimensions": dimensions.len(),
        "cases_outline": cases_outline.len(),
    })))
}

async fn action_add_analysis_note(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("risk")
        .to_ascii_lowercase();
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Analysis note")
        .to_string();
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let severity = args
        .get("severity")
        .and_then(|v| v.as_str())
        .map(String::from);
    let evidence_refs: Vec<String> = args
        .get("evidence_refs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let note_id = format!("note-{}", Uuid::new_v4().simple());
    let event = ReportEvent::AddAnalysisNote {
        run_id: run_id.clone(),
        note_id: note_id.clone(),
        category,
        title: redact_str(&title),
        description: redact_str(&description),
        severity,
        evidence_refs: redact_string_vec(&evidence_refs),
        recorded_at: timestamp_now(),
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "note_id": note_id,
    })))
}

async fn action_add_runbook_section(args: &Value) -> anyhow::Result<ToolResult> {
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return Ok(err("missing 'run_id'")),
    };
    let section_kind = args
        .get("section_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("sop_steps")
        .to_ascii_lowercase();
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Runbook section")
        .to_string();
    let body = args
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if body.is_empty() {
        return Ok(err("add_runbook_section requires non-empty 'body'"));
    }
    let section_id = format!("section-{}", Uuid::new_v4().simple());
    let event = ReportEvent::AddRunbookSection {
        run_id: run_id.clone(),
        section_id: section_id.clone(),
        section_kind,
        title: redact_str(&title),
        body: redact_str(&body),
        recorded_at: timestamp_now(),
    };
    append_event(&run_id, &event).await?;
    Ok(ok(json!({
        "run_id": run_id,
        "section_id": section_id,
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
    let raw_report = render_report(&events, summary_note.as_deref());

    let (sanitized_report, pii_report) =
        crate::services::pii_sanitizer::global_sanitizer().sanitize(&raw_report);
    let report_md = inject_pii_summary(&sanitized_report, &pii_report);

    let report_path = run_dir(&run_id).join("report.md");
    if let Some(parent) = report_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&report_path, &report_md).await?;
    let report_path_str = report_path.to_string_lossy().to_string();

    let raw_tech_doc = render_tech_doc(&events, summary_note.as_deref());
    let (sanitized_tech_doc, _) =
        crate::services::pii_sanitizer::global_sanitizer().sanitize(&raw_tech_doc);
    let tech_doc_path = run_dir(&run_id).join("tech_doc.md");
    tokio::fs::write(&tech_doc_path, &sanitized_tech_doc).await?;
    let tech_doc_path_str = tech_doc_path.to_string_lossy().to_string();

    let raw_analysis = render_analysis(&events, summary_note.as_deref());
    let (sanitized_analysis, _) =
        crate::services::pii_sanitizer::global_sanitizer().sanitize(&raw_analysis);
    let analysis_path = run_dir(&run_id).join("analysis.md");
    tokio::fs::write(&analysis_path, &sanitized_analysis).await?;
    let analysis_path_str = analysis_path.to_string_lossy().to_string();

    let raw_runbook = render_runbook(&events, summary_note.as_deref());
    let (sanitized_runbook, _) =
        crate::services::pii_sanitizer::global_sanitizer().sanitize(&raw_runbook);
    let runbook_path = run_dir(&run_id).join("runbook.md");
    tokio::fs::write(&runbook_path, &sanitized_runbook).await?;
    let runbook_path_str = runbook_path.to_string_lossy().to_string();

    let event = ReportEvent::Finalize {
        run_id: run_id.clone(),
        summary_note: summary_note.as_deref().map(redact_str),
        finalized_at: timestamp_now(),
        report_path: report_path_str.clone(),
    };
    append_event(&run_id, &event).await?;
    if active_run_id().as_deref() == Some(run_id.as_str()) {
        set_active_run_id(None);
    }

    let mut pii_counts = serde_json::Map::new();
    for (kind, count) in pii_report.counts.iter() {
        pii_counts.insert(
            kind.label().to_string(),
            serde_json::Value::from(*count as u64),
        );
    }

    Ok(ok(json!({
        "run_id": run_id,
        "report_path": report_path_str,
        "tech_doc_path": tech_doc_path_str,
        "analysis_path": analysis_path_str,
        "runbook_path": runbook_path_str,
        "relative_path": normalize_md_path(
            &report_path
                .strip_prefix(workspace_anchor())
                .unwrap_or(&report_path)
                .to_string_lossy(),
        ),
        "tech_doc_relative_path": normalize_md_path(
            &tech_doc_path
                .strip_prefix(workspace_anchor())
                .unwrap_or(&tech_doc_path)
                .to_string_lossy(),
        ),
        "analysis_relative_path": normalize_md_path(
            &analysis_path
                .strip_prefix(workspace_anchor())
                .unwrap_or(&analysis_path)
                .to_string_lossy(),
        ),
        "runbook_relative_path": normalize_md_path(
            &runbook_path
                .strip_prefix(workspace_anchor())
                .unwrap_or(&runbook_path)
                .to_string_lossy(),
        ),
        "pii_redacted": {
            "total": pii_report.total(),
            "counts": serde_json::Value::Object(pii_counts),
        },
    })))
}

fn inject_pii_summary(
    report_md: &str,
    pii_report: &crate::services::pii_sanitizer::SanitizationReport,
) -> String {
    let mut header = String::new();
    header.push_str("> 隐私脱敏 (PII Redaction): ");
    if pii_report.is_empty() {
        header.push_str("none detected.\n\n");
    } else {
        header.push_str(&format!("{} item(s) replaced with stable placeholders before write.\n\n", pii_report.total()));
        header.push_str("| Category | Count |\n");
        header.push_str("|----------|-------|\n");
        let mut entries: Vec<_> = pii_report.counts.iter().collect();
        entries.sort_by(|a, b| a.0.label().cmp(b.0.label()));
        for (kind, count) in entries {
            header.push_str(&format!("| {} | {} |\n", kind.label(), count));
        }
        header.push('\n');
    }

    if let Some(idx) = report_md.find("\n## ") {
        let mut out = String::with_capacity(report_md.len() + header.len());
        out.push_str(&report_md[..idx]);
        out.push('\n');
        out.push_str(&header);
        out.push_str(&report_md[idx + 1..]);
        out
    } else {
        format!("{report_md}\n{header}")
    }
}

fn render_tech_doc(events: &[ReportEvent], summary_note: Option<&str>) -> String {
    let mut title = String::from("Debug QA – Technical Documentation");
    let mut started_at = String::new();
    let mut target_urls: Vec<String> = Vec::new();
    let mut run_id_value = String::new();
    let mut findings: Vec<FindingRow> = Vec::new();
    let mut coverage_rows: Vec<CoverageRow> = Vec::new();
    let mut standalone_network_errors: Vec<(String, i64, Option<String>, Option<String>, String)> = Vec::new();
    let mut interactions: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut console_groups: usize = 0;
    let mut screenshots: Vec<ScreenshotRow> = Vec::new();

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
                title = format!("{} – Technical Documentation", t);
                target_urls = urls.clone();
                started_at = ts.clone();
            }
            ReportEvent::AddCase {
                title: ctitle,
                status,
                steps,
                ..
            } => {
                interactions.push((ctitle.clone(), status.clone(), steps.clone()));
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
                if let Some(slot) = coverage_rows.iter_mut().find(|r| r.url == row.url) {
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
            ReportEvent::AttachConsoleLogs { .. } => {
                console_groups += 1;
            }
            ReportEvent::AttachScreenshot {
                attachment_id,
                relative_path,
                caption,
                step_ref,
                recorded_at,
                ..
            } => {
                screenshots.push(ScreenshotRow {
                    id: attachment_id.clone(),
                    relative_path: relative_path.clone(),
                    caption: caption.clone(),
                    step_ref: step_ref.clone(),
                    recorded_at: recorded_at.clone(),
                });
            }
            ReportEvent::AddTestPlan { .. } => {}
            ReportEvent::AddAnalysisNote { .. } => {}
            ReportEvent::AddRunbookSection { .. } => {}
            _ => {}
        }
    }

    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    out.push_str(&format!("- Run ID: `{}`\n", run_id_value));
    if !started_at.is_empty() {
        out.push_str(&format!("- Generated: {}\n", started_at));
    }
    if let Some(note) = summary_note {
        if !note.is_empty() {
            out.push_str(&format!("\n> {}\n", note));
        }
    }
    out.push_str("\n本文档由 `debug_test_report` 工具在 finalize 阶段自动生成，描述被测系统的页面拓扑、API 痕迹、关键交互流程和已知风险。所有敏感数据已通过本地 PII 脱敏层替换为占位符，未提交给 LLM。\n");

    out.push_str("\n## 1. 被测目标\n\n");
    if target_urls.is_empty() {
        out.push_str("- (no explicit targets recorded)\n");
    } else {
        for url in &target_urls {
            out.push_str(&format!("- {}\n", url));
        }
    }

    out.push_str("\n## 2. 页面/路由地图\n\n");
    if coverage_rows.is_empty() {
        out.push_str("(本次 run 未通过 add_coverage_entry 上报覆盖数据)\n");
    } else {
        let mut origins: std::collections::BTreeMap<String, Vec<&CoverageRow>> =
            std::collections::BTreeMap::new();
        for row in &coverage_rows {
            let origin = extract_origin(&row.url).unwrap_or_else(|| "(unknown)".into());
            origins.entry(origin).or_default().push(row);
        }
        for (origin, rows) in &origins {
            out.push_str(&format!("### {}\n\n", origin));
            for row in rows {
                let title_part = row.title.as_deref().unwrap_or("");
                let suffix = if title_part.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", title_part)
                };
                out.push_str(&format!(
                    "- (depth {}) {}{} [http={}]\n",
                    row.depth,
                    row.url,
                    suffix,
                    row
                        .http_status
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "-".into())
                ));
            }
            out.push('\n');
        }
    }

    out.push_str("\n## 3. API / 网络观测\n\n");
    let mut api_lines: Vec<String> = Vec::new();
    for row in &coverage_rows {
        for entry in &row.network_errors {
            if let Some(obj) = entry.as_object() {
                let url = obj
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                let status = obj.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
                let method = obj.get("method").and_then(|v| v.as_str()).unwrap_or("-");
                api_lines.push(format!(
                    "- `{}` `{}` -> {} (page: {})",
                    method, url, status, row.url
                ));
            }
        }
    }
    for (url, status, method, page_url, _) in &standalone_network_errors {
        api_lines.push(format!(
            "- `{}` `{}` -> {} (page: {})",
            method.as_deref().unwrap_or("-"),
            url,
            status,
            page_url.as_deref().unwrap_or("-")
        ));
    }
    if api_lines.is_empty() {
        out.push_str("(no failing or notable network calls captured)\n");
    } else {
        for line in &api_lines {
            out.push_str(line);
            out.push('\n');
        }
    }

    out.push_str("\n## 4. 关键交互流程\n\n");
    if interactions.is_empty() {
        out.push_str("(no test cases recorded)\n");
    } else {
        for (i, (title, status, steps)) in interactions.iter().enumerate() {
            out.push_str(&format!(
                "### Flow {}: {} `[{}]`\n\n",
                i + 1,
                title,
                status
            ));
            if steps.is_empty() {
                out.push_str("(no steps)\n");
            } else {
                for (idx, step) in steps.iter().enumerate() {
                    out.push_str(&format!("{}. {}\n", idx + 1, step));
                }
            }
            out.push('\n');
        }
    }

    out.push_str("\n## 5. 风险与建议\n\n");
    if findings.is_empty() {
        out.push_str("(no findings recorded)\n");
    } else {
        let mut by_sev: std::collections::BTreeMap<String, Vec<&FindingRow>> =
            std::collections::BTreeMap::new();
        for f in &findings {
            by_sev.entry(f.severity.clone()).or_default().push(f);
        }
        for (sev, list) in &by_sev {
            out.push_str(&format!("### Severity: {}\n\n", sev));
            for f in list {
                out.push_str(&format!("- **{}** ({}): {}\n", f.id, f.title, f.description));
                if let Some(rc) = &f.root_cause {
                    out.push_str(&format!("  - Root cause: {}\n", rc));
                }
                if let Some(fix) = &f.fix_suggestion {
                    out.push_str(&format!("  - Suggested fix: {}\n", fix));
                }
                if let Some(cat) = &f.category {
                    out.push_str(&format!("  - Category: {}\n", cat));
                }
            }
            out.push('\n');
        }
    }

    out.push_str("\n## 6. Screenshots\n\n");
    if screenshots.is_empty() {
        out.push_str("(no screenshots attached)\n");
    } else {
        let mut by_case: std::collections::BTreeMap<String, Vec<&ScreenshotRow>> =
            std::collections::BTreeMap::new();
        for s in &screenshots {
            let key = s
                .step_ref
                .clone()
                .unwrap_or_else(|| "ungrouped".to_string());
            by_case.entry(key).or_default().push(s);
        }
        for (case_id, shots) in &by_case {
            out.push_str(&format!("### {}\n\n", case_id));
            for s in shots {
                let caption = s.caption.clone().unwrap_or_else(|| s.id.clone());
                out.push_str(&format!(
                    "- ![{caption}]({path}) — `{id}` @ {ts}\n",
                    caption = caption,
                    path = s.relative_path,
                    id = s.id,
                    ts = s.recorded_at,
                ));
            }
            out.push('\n');
        }
    }

    out.push_str("\n## 7. 观测附录\n\n");
    out.push_str(&format!("- 覆盖页面数: {}\n", coverage_rows.len()));
    out.push_str(&format!("- API 失败条目: {}\n", api_lines.len()));
    out.push_str(&format!("- 控制台日志组: {}\n", console_groups));
    out.push_str(&format!("- Screenshots: {}\n", screenshots.len()));
    out.push_str(&format!("- Findings: {}\n", findings.len()));

    out
}

fn render_analysis(events: &[ReportEvent], summary_note: Option<&str>) -> String {
    let mut title = String::from("Debug QA Analysis");
    let mut started_at = String::new();
    let mut run_id_value = String::new();
    let mut findings: Vec<FindingRow> = Vec::new();
    let mut notes: Vec<AnalysisNoteRow> = Vec::new();

    for event in events {
        match event {
            ReportEvent::Start {
                run_id,
                title: t,
                started_at: ts,
                ..
            } => {
                run_id_value = run_id.clone();
                title = format!("{} – Analysis", t);
                started_at = ts.clone();
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
            ReportEvent::AddAnalysisNote {
                note_id,
                category,
                title,
                description,
                severity,
                evidence_refs,
                ..
            } => {
                notes.push(AnalysisNoteRow {
                    id: note_id.clone(),
                    category: category.clone(),
                    title: title.clone(),
                    description: description.clone(),
                    severity: severity.clone(),
                    evidence_refs: evidence_refs.clone(),
                });
            }
            _ => {}
        }
    }

    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    out.push_str(&format!("- Run ID: `{}`\n", run_id_value));
    if !started_at.is_empty() {
        out.push_str(&format!("- Generated: {}\n", started_at));
    }
    if let Some(note) = summary_note {
        if !note.is_empty() {
            out.push_str(&format!("\n> {}\n", note));
        }
    }
    out.push_str(
        "\n本分析报告由 `debug_test_report.finalize` 阶段从 `add_analysis_note` 与 `add_finding` 事件聚合生成，分组维度：根因 / 性能 / 安全 / a11y / UX / 风险。所有敏感数据已在落盘前完成 PII 脱敏。\n",
    );

    let categories: &[(&str, &str)] = &[
        ("root_cause", "根因"),
        ("performance", "性能"),
        ("security", "安全"),
        ("a11y", "可访问性 (a11y)"),
        ("ux", "用户体验 (UX)"),
        ("risk", "风险"),
    ];
    for (key, label) in categories {
        let matched: Vec<&AnalysisNoteRow> = notes
            .iter()
            .filter(|n| n.category == *key)
            .collect();
        let matched_findings: Vec<&FindingRow> = findings
            .iter()
            .filter(|f| match *key {
                "root_cause" => f.root_cause.is_some(),
                "performance" => f.category.as_deref() == Some("performance"),
                "security" => f.category.as_deref() == Some("security"),
                "a11y" => f.category.as_deref() == Some("access"),
                "ux" => f.category.as_deref() == Some("ui"),
                "risk" => true,
                _ => false,
            })
            .collect();
        if matched.is_empty() && matched_findings.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {}\n\n", label));
        for note in &matched {
            let sev_suffix = match &note.severity {
                Some(s) if !s.is_empty() => format!(" `[{}]`", s),
                _ => String::new(),
            };
            out.push_str(&format!("### {}{}\n\n{}\n", note.title, sev_suffix, note.description));
            if !note.evidence_refs.is_empty() {
                out.push_str("\n**Evidence**\n\n");
                for ev in &note.evidence_refs {
                    out.push_str(&format!("- {}\n", ev));
                }
            }
            out.push('\n');
        }
        if !matched_findings.is_empty() && key == &"risk" {
            out.push_str("### 相关 findings\n\n");
            for f in &matched_findings {
                out.push_str(&format!(
                    "- `{}` `[{}]` {} — {}\n",
                    f.id, f.severity, f.title, f.description
                ));
            }
        }
    }

    if notes.is_empty() && findings.is_empty() {
        out.push_str("\n(本次 run 未提交任何 analysis_note 或 finding)\n");
    }

    out
}

fn render_runbook(events: &[ReportEvent], summary_note: Option<&str>) -> String {
    let mut title = String::from("Debug QA Runbook");
    let mut started_at = String::new();
    let mut run_id_value = String::new();
    let mut sections: Vec<RunbookSectionRow> = Vec::new();

    for event in events {
        match event {
            ReportEvent::Start {
                run_id,
                title: t,
                started_at: ts,
                ..
            } => {
                run_id_value = run_id.clone();
                title = format!("{} – Runbook", t);
                started_at = ts.clone();
            }
            ReportEvent::AddRunbookSection {
                section_id,
                section_kind,
                title,
                body,
                ..
            } => {
                sections.push(RunbookSectionRow {
                    id: section_id.clone(),
                    kind: section_kind.clone(),
                    title: title.clone(),
                    body: body.clone(),
                });
            }
            _ => {}
        }
    }

    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", title));
    out.push_str(&format!("- Run ID: `{}`\n", run_id_value));
    if !started_at.is_empty() {
        out.push_str(&format!("- Generated: {}\n", started_at));
    }
    if let Some(note) = summary_note {
        if !note.is_empty() {
            out.push_str(&format!("\n> {}\n", note));
        }
    }
    out.push_str(
        "\n本操作文档由 `debug_test_report.finalize` 阶段从 `add_runbook_section` 事件聚合生成；用于人工回归、复现缺陷或回访本次测试范围。\n",
    );

    let order: &[(&str, &str)] = &[
        ("context", "场景背景"),
        ("preconditions", "前置条件"),
        ("test_data", "测试数据"),
        ("sop_steps", "操作步骤 (SOP)"),
        ("expected", "预期结果"),
        ("regression_checklist", "回归 checklist"),
        ("troubleshooting", "故障处理"),
    ];
    for (key, label) in order {
        let matched: Vec<&RunbookSectionRow> = sections
            .iter()
            .filter(|s| s.kind == *key)
            .collect();
        if matched.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {}\n\n", label));
        for s in &matched {
            if !s.title.is_empty() {
                out.push_str(&format!("### {}\n\n", s.title));
            }
            out.push_str(&s.body);
            if !s.body.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
    }

    if sections.is_empty() {
        out.push_str("\n(本次 run 未提交任何 runbook_section)\n");
    }

    out
}

struct AnalysisNoteRow {
    id: String,
    category: String,
    title: String,
    description: String,
    severity: Option<String>,
    evidence_refs: Vec<String>,
}

struct RunbookSectionRow {
    id: String,
    kind: String,
    title: String,
    body: String,
}

struct ScreenshotRow {
    id: String,
    relative_path: String,
    caption: Option<String>,
    step_ref: Option<String>,
    recorded_at: String,
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
    let mut browser_trace: Vec<BrowserTraceRow> = Vec::new();
    let mut test_plan_dimensions: Vec<Value> = Vec::new();
    let mut test_plan_outlines: Vec<Value> = Vec::new();

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
            ReportEvent::BrowserAction {
                action,
                tab_id,
                url,
                selector,
                success,
                recorded_at,
                ..
            } => {
                browser_trace.push(BrowserTraceRow {
                    action: action.clone(),
                    tab_id: *tab_id,
                    url: url.clone(),
                    selector: selector.clone(),
                    success: *success,
                    recorded_at: recorded_at.clone(),
                });
            }
            ReportEvent::AddTestPlan {
                dimensions: dims,
                cases_outline: outlines,
                ..
            } => {
                test_plan_dimensions = dims.clone();
                test_plan_outlines = outlines.clone();
            }
            ReportEvent::AddAnalysisNote { .. } => {}
            ReportEvent::AddRunbookSection { .. } => {}
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

    if !test_plan_dimensions.is_empty() || !test_plan_outlines.is_empty() {
        out.push_str("\n## 测试范围与计划\n\n");
        if !test_plan_dimensions.is_empty() {
            out.push_str("### 维度\n\n");
            out.push_str("| # | Name | Scope | Priority |\n");
            out.push_str("|---|------|-------|----------|\n");
            for (idx, dim) in test_plan_dimensions.iter().enumerate() {
                let name = dim
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unnamed)")
                    .replace('|', "\\|");
                let scope = dim
                    .get("scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .replace('|', "\\|");
                let prio = dim
                    .get("priority")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .replace('|', "\\|");
                out.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    idx + 1,
                    name,
                    scope,
                    prio
                ));
            }
        }
        if !test_plan_outlines.is_empty() {
            out.push_str("\n### 用例 outline\n\n");
            out.push_str("| # | Title | Dimension | Severity |\n");
            out.push_str("|---|-------|-----------|----------|\n");
            for (idx, c) in test_plan_outlines.iter().enumerate() {
                let title = c
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(untitled)")
                    .replace('|', "\\|");
                let dim = c
                    .get("dimension")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .replace('|', "\\|");
                let sev = c
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .replace('|', "\\|");
                out.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    idx + 1,
                    title,
                    dim,
                    sev
                ));
            }
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

    if !browser_trace.is_empty() {
        out.push_str("\n## Browser Trace\n\n");
        out.push_str("Auto-captured from `browser` tool calls during this run.\n\n");
        out.push_str("| # | Time | Action | Tab | URL | Selector | Result |\n");
        out.push_str("|---|------|--------|-----|-----|----------|--------|\n");
        for (idx, row) in browser_trace.iter().enumerate() {
            let url_cell = row
                .url
                .as_deref()
                .unwrap_or("-")
                .replace('|', "\\|");
            let selector_cell = row
                .selector
                .as_deref()
                .unwrap_or("-")
                .replace('|', "\\|");
            let tab_cell = row
                .tab_id
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".to_string());
            let result_cell = if row.success { "ok" } else { "fail" };
            out.push_str(&format!(
                "| {} | {} | `{}` | {} | {} | {} | {} |\n",
                idx + 1,
                row.recorded_at,
                row.action,
                tab_cell,
                url_cell,
                selector_cell,
                result_cell,
            ));
        }
    }

    out
}

struct BrowserTraceRow {
    action: String,
    tab_id: Option<u64>,
    url: Option<String>,
    selector: Option<String>,
    success: bool,
    recorded_at: String,
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
