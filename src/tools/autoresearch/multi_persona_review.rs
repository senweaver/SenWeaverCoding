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

const PER_FILE_BYTES: usize = 8_192;
const MAX_FILES: usize = 24;
const CONTEXT_CHAR_CAP: usize = 24_000;

pub struct MultiPersonaReviewTool {
    runtime: Arc<AutoresearchRuntime>,
}

impl MultiPersonaReviewTool {
    pub fn new(runtime: Arc<AutoresearchRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for MultiPersonaReviewTool {
    fn name(&self) -> &str {
        "multi_persona_review"
    }

    fn description(&self) -> &str {
        "Run a five (or eight) expert-persona parallel review over a scope of files. Default \
         personas: Software Architect, Security Analyst, Performance Engineer, Reliability \
         Engineer, Devil's Advocate. Pass mode='adversarial' for the hostile reviewer set \
         (Breaker, Cheater, Scaler, Newbie, Malicious Insider). Each persona analyses \
         independently; the synthesizer deduplicates, runs an anti-herd check, and ranks \
         findings by severity × persona agreement. Output is a structured markdown report \
         under `.senweavercoding/autoresearch/predict-<timestamp>/`. Use BEFORE risky \
         refactors, security-sensitive merges, or design decisions to surface blind spots."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "What the personas should focus on (e.g. 'review the new auth flow', 'evaluate the curator pipeline rewrite')."
                },
                "scope": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Workspace-relative file paths, directory paths, or glob patterns. Files outside the workspace are ignored. Example: ['src/agent/loop_.rs', 'src/gateway/**/*.rs']."
                },
                "context_text": {
                    "type": "string",
                    "description": "Optional free-form context to attach to each persona prompt (recent diff, design notes, etc.). Used in addition to or instead of `scope`."
                },
                "mode": {
                    "type": "string",
                    "enum": ["default", "adversarial"],
                    "description": "default = 5 expert personas; adversarial = 5 hostile reviewers. Defaults to 'default'."
                },
                "max_findings_per_persona": {
                    "type": "integer",
                    "description": "Cap on findings each persona reports (1-20). Defaults to 6.",
                    "minimum": 1,
                    "maximum": 20
                },
                "slug": {
                    "type": "string",
                    "description": "Optional human-readable label appended to the report directory name."
                }
            },
            "required": ["goal"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self
            .runtime
            .enforce(ToolOperation::Act, "multi_persona_review")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let goal = args
            .get("goal")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(goal) = goal else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing required 'goal' string".to_string()),
            });
        };
        let raw_scope: Vec<String> = args
            .get("scope")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let context_text = args
            .get("context_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_ascii_lowercase();
        let adversarial = mode == "adversarial";
        let max_findings = args
            .get("max_findings_per_persona")
            .and_then(|v| v.as_u64())
            .unwrap_or(6)
            .clamp(1, 20) as usize;
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

        let personas: Vec<PersonaDefinition> = if adversarial {
            adversarial_personas()
        } else {
            default_personas()
        };

        let common_user_prompt =
            build_common_user_prompt(goal, &context_text, &scope_context, max_findings);
        let tasks: Vec<PersonaTask> = personas
            .iter()
            .map(|p| PersonaTask {
                id: p.id.clone(),
                label: p.label.clone(),
                system_prompt: p.system_prompt.clone(),
                user_prompt: common_user_prompt.clone(),
            })
            .collect();

        let outcomes = fan_out_personas(&self.runtime, tasks, None, None).await;

        let mut persona_reports: Vec<PersonaReport> = Vec::new();
        for outcome in outcomes {
            let parse = parse_persona_response(&outcome.raw_response, &outcome.label);
            persona_reports.push(PersonaReport {
                id: outcome.id,
                label: outcome.label,
                summary: parse
                    .summary
                    .unwrap_or_else(|| "<no summary returned>".to_string()),
                findings: parse.findings,
                raw_response: outcome.raw_response,
                error: outcome.error,
                elapsed_ms: outcome.elapsed_ms,
            });
        }

        let consensus = synthesize_consensus(&persona_reports);

        let summary_md = render_summary_md(goal, adversarial, &persona_reports, &consensus);
        let debate_md = render_debate_md(&persona_reports);
        let report_dir = {
            let workspace = workspace.clone();
            let slug_hint = slug_hint.map(str::to_string);
            match tokio::task::spawn_blocking(move || -> Result<std::path::PathBuf, std::io::Error> {
                let report_dir = ensure_report_dir(&workspace, "predict", slug_hint.as_deref())?;
                let _ = write_text(&report_dir.join("summary.md"), &summary_md);
                let _ = write_text(&report_dir.join("debate.md"), &debate_md);
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

        let summary_path = report_dir.join("summary.md");

        let console_md = render_console_md(goal, adversarial, &consensus, &summary_path);
        let envelope = render_envelope("predict", &console_md);
        Ok(ToolResult {
            success: true,
            output: envelope,
            error: None,
        })
    }
}

struct PersonaDefinition {
    id: String,
    label: String,
    system_prompt: String,
}

fn default_personas() -> Vec<PersonaDefinition> {
    vec![
        PersonaDefinition {
            id: "architect".to_string(),
            label: "Software Architect".to_string(),
            system_prompt: persona_system_prompt(
                "Software Architect",
                "system design, component boundaries, data flow, scalability under 10x growth",
                "Watch for god classes, circular dependencies, leaky abstractions, shared mutable state, and hidden coupling.",
            ),
        },
        PersonaDefinition {
            id: "security".to_string(),
            label: "Security Analyst".to_string(),
            system_prompt: persona_system_prompt(
                "Security Analyst",
                "attack surfaces, auth/authz, data protection, injection vectors, trust boundaries",
                "Watch for raw SQL, missing authz checks, hardcoded secrets, unsanitized user input, insecure deserialization, weak crypto.",
            ),
        },
        PersonaDefinition {
            id: "performance".to_string(),
            label: "Performance Engineer".to_string(),
            system_prompt: persona_system_prompt(
                "Performance Engineer",
                "latency, throughput, resource usage, algorithmic complexity, hot paths",
                "Watch for N+1 queries, unbounded loops, missing indexes, sync I/O in hot async paths, gratuitous clones / allocations.",
            ),
        },
        PersonaDefinition {
            id: "reliability".to_string(),
            label: "Reliability Engineer".to_string(),
            system_prompt: persona_system_prompt(
                "Reliability Engineer",
                "error handling, failure modes, observability, recovery, retries, circuit breakers",
                "Watch for swallowed errors, missing retries, no circuit breakers, silent failures, missing structured logging.",
            ),
        },
        PersonaDefinition {
            id: "devils_advocate".to_string(),
            label: "Devil's Advocate".to_string(),
            system_prompt: persona_system_prompt(
                "Devil's Advocate",
                "untested assumptions, edge cases, hidden complexity, premature abstraction, maintainability",
                "Watch for happy-path-only design, untested invariants, complexity without justification, over-engineering, missing fallbacks.",
            ),
        },
    ]
}

fn adversarial_personas() -> Vec<PersonaDefinition> {
    vec![
        PersonaDefinition {
            id: "breaker".to_string(),
            label: "The Breaker".to_string(),
            system_prompt: persona_system_prompt(
                "The Breaker",
                "concrete crash / corruption / data-loss scenarios reachable from external inputs",
                "Pick one entry point and describe the exact payload that crashes / corrupts state. Cite file:line.",
            ),
        },
        PersonaDefinition {
            id: "cheater".to_string(),
            label: "The Cheater".to_string(),
            system_prompt: persona_system_prompt(
                "The Cheater",
                "rule-bypass / abuse scenarios that let a user gain unintended capabilities",
                "Find the loophole that lets someone unlock a feature they should not have access to.",
            ),
        },
        PersonaDefinition {
            id: "scaler".to_string(),
            label: "The Scaler".to_string(),
            system_prompt: persona_system_prompt(
                "The Scaler",
                "1000x load, large inputs, unbounded queues, slow downstreams, retry storms",
                "Predict the first thing that snaps under 1000x users / data / concurrent requests.",
            ),
        },
        PersonaDefinition {
            id: "newbie".to_string(),
            label: "The Newbie".to_string(),
            system_prompt: persona_system_prompt(
                "The Newbie",
                "API misuse, missing/confusing docs, footguns, bad defaults",
                "Spot every public API that an unfamiliar caller is most likely to misuse.",
            ),
        },
        PersonaDefinition {
            id: "insider".to_string(),
            label: "The Malicious Insider".to_string(),
            system_prompt: persona_system_prompt(
                "The Malicious Insider",
                "data exfiltration, lateral movement, privilege escalation given valid credentials",
                "Assume the attacker already has a low-privilege session  -  what is reachable next?",
            ),
        },
    ]
}

fn persona_system_prompt(label: &str, focus: &str, red_flags: &str) -> String {
    format!(
        "You are the {label} persona on a multi-reviewer red-team panel. You analyse the \
         provided code/context independently of other personas  -  do NOT defer to them or \
         soften your stance. Your sole focus area: {focus}.\n\
         Red-flag heuristics: {red_flags}\n\
         OUTPUT FORMAT: respond with ONE JSON object only, no markdown fences and no \
         commentary. Schema:\n\
         {{\n  \"persona\": \"{label}\",\n  \"summary\": \"<1-3 sentence assessment>\",\n  \
           \"findings\": [\n    {{\n      \"title\": \"<short imperative>\",\n      \
             \"severity\": \"critical|high|medium|low|info\",\n      \"confidence\": 0-100,\n      \
             \"file_line\": \"<path:line or empty if global>\",\n      \"evidence\": \"<1-3 sentences quoting code, behaviour, or rationale>\",\n      \
             \"recommendation\": \"<concrete next step>\"\n    }}\n  ]\n}}\n\
         Strictly cap findings at the budget you are told. NEVER invent file paths  -  \
         if you cannot cite a path, write an empty string for `file_line`. Every \
         finding MUST include `evidence`."
    )
}

fn build_common_user_prompt(
    goal: &str,
    context_text: &str,
    scope_context: &str,
    max_findings: usize,
) -> String {
    let extra = if context_text.trim().is_empty() {
        String::new()
    } else {
        format!("Additional context provided by the orchestrator:\n{context_text}\n\n")
    };
    format!(
        "Reviewer task\n-------------\n\
         Goal: {goal}\n\
         Findings budget: at most {max_findings} findings.\n\n\
         {extra}\
         In-scope code samples (truncated):\n```\n{scope_context}\n```\n\n\
         Respond now with your JSON object."
    )
}

#[derive(Debug, Clone)]
struct PersonaFinding {
    title: String,
    severity: String,
    confidence: u8,
    file_line: String,
    evidence: String,
    recommendation: String,
}

#[derive(Debug, Default)]
struct PersonaResponseParse {
    summary: Option<String>,
    findings: Vec<PersonaFinding>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PersonaReport {
    id: String,
    label: String,
    summary: String,
    findings: Vec<PersonaFinding>,
    raw_response: String,
    error: Option<String>,
    elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConsensusFinding {
    title: String,
    severity: String,
    confidence_avg: u8,
    agreement: u8,
    file_line: String,
    sources: Vec<String>,
    recommendation: String,
}

fn parse_persona_response(raw: &str, label: &str) -> PersonaResponseParse {
    let mut parse = PersonaResponseParse::default();
    if raw.trim().is_empty() {
        return parse;
    }
    let block = extract_json_block(raw).unwrap_or(raw);
    let value: Value = match serde_json::from_str(block) {
        Ok(v) => v,
        Err(_) => {
            parse.summary = Some(format!(
                "[{label}] returned non-JSON text; first 280 chars: {}",
                raw.chars().take(280).collect::<String>()
            ));
            return parse;
        }
    };
    parse.summary = value
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());
    if let Some(arr) = value.get("findings").and_then(|v| v.as_array()) {
        for item in arr {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let severity = item
                .get("severity")
                .and_then(|v| v.as_str())
                .map(parse_severity)
                .unwrap_or_else(|| "medium".to_string());
            let confidence = item
                .get("confidence")
                .and_then(|v| v.as_u64())
                .unwrap_or(50)
                .clamp(0, 100) as u8;
            let file_line = item
                .get("file_line")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let evidence = item
                .get("evidence")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let recommendation = item
                .get("recommendation")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            parse.findings.push(PersonaFinding {
                title,
                severity,
                confidence,
                file_line,
                evidence,
                recommendation,
            });
        }
    }
    parse
}

fn synthesize_consensus(reports: &[PersonaReport]) -> Vec<ConsensusFinding> {
    let mut grouped: HashMap<String, Vec<(String, PersonaFinding)>> = HashMap::new();
    for report in reports {
        for finding in &report.findings {
            let key = format!(
                "{}|{}",
                finding.title.to_ascii_lowercase().trim(),
                finding.file_line.to_ascii_lowercase().trim()
            );
            grouped
                .entry(key)
                .or_default()
                .push((report.label.clone(), finding.clone()));
        }
    }
    let mut consensus: Vec<ConsensusFinding> = grouped
        .into_values()
        .map(|group| {
            let agreement = group.len() as u8;
            let highest = group
                .iter()
                .map(|(_, f)| severity_rank(&f.severity))
                .max()
                .unwrap_or(0);
            let severity = match highest {
                5 => "critical",
                4 => "high",
                3 => "medium",
                2 => "low",
                1 => "info",
                _ => "medium",
            }
            .to_string();
            let confidence_sum: u32 = group.iter().map(|(_, f)| f.confidence as u32).sum();
            let confidence_avg = (confidence_sum / group.len() as u32).clamp(0, 100) as u8;
            let representative = group
                .iter()
                .max_by_key(|(_, f)| severity_rank(&f.severity) as u32 * 200 + f.confidence as u32)
                .map(|(_, f)| f.clone())
                .unwrap_or_else(|| group[0].1.clone());
            let sources: Vec<String> = group.iter().map(|(label, _)| label.clone()).collect();
            ConsensusFinding {
                title: representative.title,
                severity,
                confidence_avg,
                agreement,
                file_line: representative.file_line,
                sources,
                recommendation: representative.recommendation,
            }
        })
        .collect();
    consensus.sort_by(|a, b| {
        let lhs = (
            severity_rank(&a.severity),
            a.agreement,
            a.confidence_avg,
        );
        let rhs = (
            severity_rank(&b.severity),
            b.agreement,
            b.confidence_avg,
        );
        rhs.cmp(&lhs)
    });
    consensus
}

fn render_summary_md(
    goal: &str,
    adversarial: bool,
    reports: &[PersonaReport],
    consensus: &[ConsensusFinding],
) -> String {
    let mode_label = if adversarial { "adversarial" } else { "default" };
    let mut out = String::new();
    out.push_str(&format!("# Multi-Persona Review ({mode_label})\n\n"));
    out.push_str(&format!("**Goal**: {goal}\n\n"));
    out.push_str(&format!("**Personas**: {}\n\n", reports.len()));
    out.push_str("## Consensus (top findings)\n\n");
    if consensus.is_empty() {
        out.push_str("_No findings produced. Review the persona transcripts in `debate.md`._\n\n");
    } else {
        out.push_str("| # | Severity | Agreement | Confidence | Title | File:Line | Sources |\n");
        out.push_str("|---|---|---|---|---|---|---|\n");
        for (idx, f) in consensus.iter().take(20).enumerate() {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | `{}` | {} |\n",
                idx + 1,
                f.severity,
                f.agreement,
                f.confidence_avg,
                escape_md(&f.title),
                escape_md(&f.file_line),
                escape_md(&f.sources.join(", ")),
            ));
        }
        out.push('\n');
        out.push_str("### Top recommendations\n\n");
        for (idx, f) in consensus.iter().take(10).enumerate() {
            out.push_str(&format!(
                "- **{}. [{}] {}**  -  {}\n",
                idx + 1,
                f.severity,
                escape_md(&f.title),
                escape_md(&f.recommendation),
            ));
        }
        out.push('\n');
    }
    out.push_str("## Per-persona summary\n\n");
    for report in reports {
        out.push_str(&format!("### {}\n", report.label));
        if let Some(err) = &report.error {
            out.push_str(&format!("> Persona call failed: {}\n\n", escape_md(err)));
            continue;
        }
        out.push_str(&format!("- Findings: {}\n", report.findings.len()));
        out.push_str(&format!("- Summary: {}\n\n", escape_md(&report.summary)));
    }
    out
}

fn render_debate_md(reports: &[PersonaReport]) -> String {
    let mut out = String::new();
    out.push_str("# Persona Debate Transcript\n\n");
    for report in reports {
        out.push_str(&format!("## {}\n\n", report.label));
        out.push_str(&format!(
            "- elapsed_ms: {}\n- findings: {}\n\n",
            report.elapsed_ms,
            report.findings.len()
        ));
        if let Some(err) = &report.error {
            out.push_str(&format!("**Error**: {}\n\n", escape_md(err)));
        }
        if !report.summary.is_empty() {
            out.push_str(&format!("**Summary**: {}\n\n", escape_md(&report.summary)));
        }
        for (idx, f) in report.findings.iter().enumerate() {
            out.push_str(&format!(
                "### Finding {}  -  [{}] {} (confidence {})\n",
                idx + 1,
                f.severity,
                escape_md(&f.title),
                f.confidence,
            ));
            if !f.file_line.is_empty() {
                out.push_str(&format!("- File: `{}`\n", escape_md(&f.file_line)));
            }
            if !f.evidence.is_empty() {
                out.push_str(&format!("- Evidence: {}\n", escape_md(&f.evidence)));
            }
            if !f.recommendation.is_empty() {
                out.push_str(&format!("- Recommendation: {}\n", escape_md(&f.recommendation)));
            }
            out.push('\n');
        }
        out.push_str("---\n\n## Raw response\n\n");
        out.push_str("```\n");
        out.push_str(&report.raw_response);
        out.push_str("\n```\n\n");
    }
    out
}

fn render_console_md(
    goal: &str,
    adversarial: bool,
    consensus: &[ConsensusFinding],
    summary_path: &std::path::Path,
) -> String {
    let mode_label = if adversarial { "adversarial" } else { "default" };
    let mut out = String::new();
    out.push_str(&format!(
        "Multi-persona review complete ({mode_label}, {} findings)\n\n",
        consensus.len()
    ));
    out.push_str(&format!("Goal: {goal}\n\n"));
    if consensus.is_empty() {
        out.push_str("No consensus findings; see persona transcripts.\n");
    } else {
        out.push_str("Top findings:\n");
        for (idx, f) in consensus.iter().take(10).enumerate() {
            out.push_str(&format!(
                "{:>2}. [{}] {}  -  `{}` (agreement {}, confidence {})\n",
                idx + 1,
                f.severity,
                escape_md(&f.title),
                escape_md(&f.file_line),
                f.agreement,
                f.confidence_avg,
            ));
        }
    }
    out.push_str(&format!(
        "\nFull report: `{}`\n",
        summary_path.display().to_string().replace('\\', "/")
    ));
    out
}

fn escape_md(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}
