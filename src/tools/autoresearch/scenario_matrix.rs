// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::helpers::{
    AutoresearchRuntime, PersonaTask, build_scope_context_snippet, collect_scope_samples,
    ensure_report_dir, extract_json_block, fan_out_personas, parse_severity, render_envelope,
    severity_rank, write_text,
};
use crate::security::policy::ToolOperation;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

const PER_FILE_BYTES: usize = 6_144;
const MAX_FILES: usize = 20;
const CONTEXT_CHAR_CAP: usize = 16_000;

pub struct ScenarioMatrixTool {
    runtime: Arc<AutoresearchRuntime>,
}

impl ScenarioMatrixTool {
    pub fn new(runtime: Arc<AutoresearchRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for ScenarioMatrixTool {
    fn name(&self) -> &str {
        "scenario_matrix"
    }

    fn description(&self) -> &str {
        "Exhaustively enumerate edge-case scenarios for a feature / flow across 12 dimensions \
         (happy path, validation, permissions, concurrency, state transitions, scale, failure \
         modes, security, integration, data shape, UX, recovery). Useful before writing tests \
         or before shipping a risky feature. Produces a categorized scenario library at \
         `.senweavercoding/autoresearch/scenario-<timestamp>/` plus an in-message summary."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "scenario": {
                    "type": "string",
                    "description": "Seed scenario or feature description (e.g. 'user opens the curator panel and starts a research task with web research disabled')."
                },
                "domain": {
                    "type": "string",
                    "description": "Optional domain hint: 'web' / 'mobile' / 'api' / 'cli' / 'data-pipeline' / 'infrastructure' / 'desktop'."
                },
                "scope": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Workspace-relative file paths, dirs, or globs that anchor the analysis. Optional."
                },
                "focus": {
                    "type": "string",
                    "description": "Optional dimension to prioritise (e.g. 'concurrency', 'security')."
                },
                "depth": {
                    "type": "string",
                    "enum": ["shallow", "standard", "deep"],
                    "description": "shallow = ~10 scenarios across few dimensions; standard = ~20 across all 12; deep = ~40 with dimension oversampling. Defaults to 'standard'."
                },
                "slug": {
                    "type": "string",
                    "description": "Optional human-readable label appended to the report directory name."
                }
            },
            "required": ["scenario"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self.runtime.enforce(ToolOperation::Act, "scenario_matrix") {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let scenario = args
            .get("scenario")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(scenario) = scenario else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing required 'scenario' string".to_string()),
            });
        };
        let domain = args
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("unspecified")
            .to_string();
        let raw_scope: Vec<String> = args
            .get("scope")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let focus = args
            .get("focus")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase());
        let depth = args
            .get("depth")
            .and_then(|v| v.as_str())
            .unwrap_or("standard");
        let slug_hint = args.get("slug").and_then(|v| v.as_str());

        let workspace = self.runtime.workspace_dir();
        let scope_samples = {
            let workspace = workspace.clone();
            let raw_scope = raw_scope.clone();
            let security = self.runtime.security.clone();
            match tokio::task::spawn_blocking(move || {
                collect_scope_samples(&workspace, &raw_scope, PER_FILE_BYTES, MAX_FILES, &security)
            })
            .await
            {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("scope collection failed: {e:#}")),
                    });
                }
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("scope collection task failed: {e}")),
                    });
                }
            }
        };
        let scope_context = if scope_samples.is_empty() {
            "<no in-scope files collected>".to_string()
        } else {
            build_scope_context_snippet(&scope_samples, CONTEXT_CHAR_CAP)
        };

        let dimensions = DIMENSIONS.to_vec();
        let target_dimensions = select_target_dimensions(&dimensions, focus.as_deref(), depth);
        let per_dim_budget = per_dimension_budget(depth);

        let tasks: Vec<PersonaTask> = target_dimensions
            .iter()
            .map(|d| PersonaTask {
                id: d.id.to_string(),
                label: d.label.to_string(),
                system_prompt: dimension_system_prompt(d),
                user_prompt: dimension_user_prompt(
                    scenario,
                    &domain,
                    &scope_context,
                    d,
                    per_dim_budget,
                ),
            })
            .collect();

        let outcomes = fan_out_personas(&self.runtime, tasks, None, None).await;

        let mut by_dimension: HashMap<String, Vec<ScenarioEntry>> = HashMap::new();
        let mut errors: Vec<(String, String)> = Vec::new();
        for outcome in outcomes {
            if let Some(err) = outcome.error.clone() {
                errors.push((outcome.label.clone(), err));
                continue;
            }
            let parsed = parse_scenario_response(&outcome.raw_response);
            by_dimension
                .entry(outcome.label.clone())
                .or_default()
                .extend(parsed);
        }
        let saturated = detect_saturated(&by_dimension);

        let total: usize = by_dimension.values().map(|v| v.len()).sum();
        let mut all_entries: Vec<(String, ScenarioEntry)> = Vec::new();
        for (label, entries) in &by_dimension {
            for entry in entries {
                all_entries.push((label.clone(), entry.clone()));
            }
        }
        all_entries.sort_by(|a, b| {
            severity_rank(&b.1.severity)
                .cmp(&severity_rank(&a.1.severity))
                .then_with(|| a.1.title.cmp(&b.1.title))
        });

        let scenarios_md = render_scenarios_md(scenario, &domain, depth, &by_dimension, &saturated);
        let edges_md = render_edge_cases_md(scenario, &all_entries);
        let report_dir = {
            let workspace = workspace.clone();
            let slug_hint = slug_hint.map(str::to_string);
            match tokio::task::spawn_blocking(move || -> Result<std::path::PathBuf, std::io::Error> {
                let report_dir = ensure_report_dir(&workspace, "scenario", slug_hint.as_deref())?;
                let _ = write_text(&report_dir.join("scenarios.md"), &scenarios_md);
                let _ = write_text(&report_dir.join("edge-cases.md"), &edges_md);
                Ok(report_dir)
            })
            .await
            {
                Ok(Ok(p)) => p,
                Ok(Err(e)) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("ensure report dir failed: {e:#}")),
                    });
                }
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("report write task failed: {e}")),
                    });
                }
            }
        };

        let console = render_console_md(
            scenario,
            &domain,
            total,
            &by_dimension,
            &saturated,
            &errors,
            &report_dir.join("scenarios.md"),
        );
        Ok(ToolResult {
            success: true,
            output: render_envelope("scenario", &console),
            error: None,
        })
    }
}

#[derive(Clone, Copy)]
struct DimensionDef {
    id: &'static str,
    label: &'static str,
    description: &'static str,
}

const DIMENSIONS: [DimensionDef; 12] = [
    DimensionDef {
        id: "happy_path",
        label: "Happy Path",
        description: "Normal successful flows that should always succeed.",
    },
    DimensionDef {
        id: "validation",
        label: "Validation",
        description: "Input boundaries, formats, types, length / range / encoding limits.",
    },
    DimensionDef {
        id: "permissions",
        label: "Permissions",
        description: "Auth, roles, access control, missing tokens, expired credentials.",
    },
    DimensionDef {
        id: "concurrency",
        label: "Concurrency",
        description: "Race conditions, deadlocks, ordering, idempotency under retries.",
    },
    DimensionDef {
        id: "state_transitions",
        label: "State Transitions",
        description: "Invalid transitions, corruption, half-applied changes, partial saves.",
    },
    DimensionDef {
        id: "scale",
        label: "Scale",
        description: "High volume, large data, many users, long-running flows, big payloads.",
    },
    DimensionDef {
        id: "failure_modes",
        label: "Failure Modes",
        description: "Network errors, timeouts, partial failures, dependency outages.",
    },
    DimensionDef {
        id: "security",
        label: "Security",
        description: "Injection, abuse, bypass, info leakage, supply-chain risks.",
    },
    DimensionDef {
        id: "integration",
        label: "Integration",
        description: "Third-party failures, API contract changes, schema drift, vendor quirks.",
    },
    DimensionDef {
        id: "data_shape",
        label: "Data Shape",
        description: "Null / empty / unicode / overflow / inconsistent types / mismatched units.",
    },
    DimensionDef {
        id: "ux",
        label: "UX",
        description: "User confusion, accidental misuse, accessibility, error messaging.",
    },
    DimensionDef {
        id: "recovery",
        label: "Recovery",
        description: "Retry, rollback, idempotency keys, replay, manual intervention paths.",
    },
];

fn select_target_dimensions(
    all: &[DimensionDef],
    focus: Option<&str>,
    depth: &str,
) -> Vec<DimensionDef> {
    let take = match depth {
        "shallow" => 6,
        "deep" => 12,
        _ => 12,
    };
    let mut chosen: Vec<DimensionDef> = Vec::new();
    if let Some(focus) = focus {
        let focus = focus.trim();
        if let Some(idx) = all
            .iter()
            .position(|d| d.id == focus || d.label.eq_ignore_ascii_case(focus))
        {
            chosen.push(all[idx]);
        }
    }
    for d in all {
        if chosen.iter().all(|c| c.id != d.id) {
            chosen.push(*d);
            if chosen.len() >= take {
                break;
            }
        }
    }
    chosen
}

fn per_dimension_budget(depth: &str) -> usize {
    match depth {
        "shallow" => 2,
        "deep" => 5,
        _ => 3,
    }
}

fn dimension_system_prompt(dim: &DimensionDef) -> String {
    format!(
        "You are the dedicated edge-case generator for the '{label}' dimension. \
         Stay strictly inside your dimension; do NOT produce scenarios that belong to another \
         dimension (e.g. a 'happy path' generator must not output 'security' scenarios).\n\
         Dimension intent: {description}\n\
         OUTPUT FORMAT: respond with ONE JSON object only, no markdown fences and no commentary:\n\
         {{\n  \"dimension\": \"{label}\",\n  \"scenarios\": [\n    {{\n      \
           \"title\": \"<short scenario name>\",\n      \"severity\": \"critical|high|medium|low|info\",\n      \
           \"preconditions\": \"<setup needed>\",\n      \"trigger\": \"<concrete action / payload>\",\n      \
           \"expected_outcome\": \"<what the system should do>\",\n      \
           \"observable\": \"<how to verify pass/fail>\"\n    }}\n  ]\n}}\n\
         Each scenario must be concrete and falsifiable. Quote literal payloads, URL paths, \
         button labels, etc. where applicable. Do not produce vague \"works correctly\" \
         scenarios  -  describe the exact failure or success criterion.",
        label = dim.label,
        description = dim.description,
    )
}

fn dimension_user_prompt(
    scenario: &str,
    domain: &str,
    scope_context: &str,
    dim: &DimensionDef,
    budget: usize,
) -> String {
    format!(
        "Seed scenario\n-------------\n{scenario}\n\n\
         Domain: {domain}\n\
         Generate at most {budget} concrete scenarios for the '{label}' dimension only.\n\n\
         In-scope code samples (truncated):\n```\n{scope_context}\n```\n\n\
         Respond now with your JSON object.",
        label = dim.label
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScenarioEntry {
    title: String,
    severity: String,
    preconditions: String,
    trigger: String,
    expected_outcome: String,
    observable: String,
}

fn parse_scenario_response(raw: &str) -> Vec<ScenarioEntry> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let block = extract_json_block(raw).unwrap_or(raw);
    let value: Value = match serde_json::from_str(block) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(arr) = value.get("scenarios").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                return None;
            }
            let severity = item
                .get("severity")
                .and_then(|v| v.as_str())
                .map(parse_severity)
                .unwrap_or_else(|| "medium".to_string());
            Some(ScenarioEntry {
                title,
                severity,
                preconditions: item
                    .get("preconditions")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                trigger: item
                    .get("trigger")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                expected_outcome: item
                    .get("expected_outcome")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                observable: item
                    .get("observable")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            })
        })
        .collect()
}

fn detect_saturated(by_dim: &HashMap<String, Vec<ScenarioEntry>>) -> Vec<String> {
    let mut saturated: Vec<String> = Vec::new();
    for (label, entries) in by_dim {
        let unique: std::collections::BTreeSet<String> = entries
            .iter()
            .map(|e| e.title.to_ascii_lowercase())
            .collect();
        if entries.len() >= 3 && unique.len() <= 1 {
            saturated.push(label.clone());
        }
    }
    saturated.sort();
    saturated
}

fn render_scenarios_md(
    scenario: &str,
    domain: &str,
    depth: &str,
    by_dim: &HashMap<String, Vec<ScenarioEntry>>,
    saturated: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("# Scenario Matrix\n\n");
    out.push_str(&format!("- Seed: {scenario}\n"));
    out.push_str(&format!("- Domain: {domain}\n"));
    out.push_str(&format!("- Depth: {depth}\n"));
    out.push_str(&format!(
        "- Dimensions covered: {}/{} (saturated: {})\n\n",
        by_dim.len(),
        DIMENSIONS.len(),
        if saturated.is_empty() {
            "none".to_string()
        } else {
            saturated.join(", ")
        }
    ));
    let mut dim_keys: Vec<&String> = by_dim.keys().collect();
    dim_keys.sort();
    for key in dim_keys {
        let entries = &by_dim[key];
        out.push_str(&format!("## {key}\n\n"));
        if entries.is_empty() {
            out.push_str("_No scenarios returned for this dimension._\n\n");
            continue;
        }
        for (idx, entry) in entries.iter().enumerate() {
            out.push_str(&format!(
                "### {}. [{}] {}\n",
                idx + 1,
                entry.severity,
                entry.title
            ));
            if !entry.preconditions.is_empty() {
                out.push_str(&format!("- Preconditions: {}\n", entry.preconditions));
            }
            if !entry.trigger.is_empty() {
                out.push_str(&format!("- Trigger: {}\n", entry.trigger));
            }
            if !entry.expected_outcome.is_empty() {
                out.push_str(&format!(
                    "- Expected outcome: {}\n",
                    entry.expected_outcome
                ));
            }
            if !entry.observable.is_empty() {
                out.push_str(&format!("- Observable: {}\n", entry.observable));
            }
            out.push('\n');
        }
    }
    out
}

fn render_edge_cases_md(scenario: &str, all: &[(String, ScenarioEntry)]) -> String {
    let mut out = String::new();
    out.push_str("# Edge Cases (severity-ranked)\n\n");
    out.push_str(&format!("Seed: {scenario}\n\n"));
    if all.is_empty() {
        out.push_str("_No scenarios generated._\n");
        return out;
    }
    out.push_str("| # | Severity | Dimension | Title |\n|---|---|---|---|\n");
    for (idx, (dim, entry)) in all.iter().enumerate() {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            idx + 1,
            entry.severity,
            dim,
            entry.title.replace('|', "\\|")
        ));
    }
    out
}

fn render_console_md(
    scenario: &str,
    domain: &str,
    total: usize,
    by_dim: &HashMap<String, Vec<ScenarioEntry>>,
    saturated: &[String],
    errors: &[(String, String)],
    summary_path: &std::path::Path,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Scenario matrix complete: {total} scenarios across {} dimensions\n",
        by_dim.len()
    ));
    out.push_str(&format!("Seed: {scenario}\nDomain: {domain}\n"));
    if !saturated.is_empty() {
        out.push_str(&format!("Saturated dimensions: {}\n", saturated.join(", ")));
    }
    if !errors.is_empty() {
        out.push_str("Errored personas:\n");
        for (label, err) in errors {
            out.push_str(&format!("- {label}: {err}\n"));
        }
    }
    out.push_str(&format!(
        "\nFull report: `{}`\n",
        summary_path.display().to_string().replace('\\', "/")
    ));
    out
}
