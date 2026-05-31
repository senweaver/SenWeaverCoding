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
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

const PER_FILE_BYTES: usize = 8_192;
const MAX_FILES: usize = 30;
const CONTEXT_CHAR_CAP: usize = 28_000;

pub struct SecurityAuditTool {
    runtime: Arc<AutoresearchRuntime>,
}

impl SecurityAuditTool {
    pub fn new(runtime: Arc<AutoresearchRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for SecurityAuditTool {
    fn name(&self) -> &str {
        "security_audit"
    }

    fn description(&self) -> &str {
        "Run a code-level security audit using the STRIDE threat model crossed with OWASP \
         Top-10, executed in parallel by four red-team personas (Security Adversary, Supply \
         Chain Attacker, Malicious Insider, Infrastructure Attacker). Every finding cites \
         file:line plus the matching STRIDE category and OWASP control. Produces \
         `findings.md`, `owasp-coverage.md`, `stride-coverage.md` under \
         `.senweavercoding/autoresearch/security-<timestamp>/`, plus a console summary. Use \
         before a release, after touching auth/IO/serialization code, or whenever the user \
         asks for a security review."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "scope": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Workspace-relative file paths, directories, or globs to audit (e.g. ['src/gateway/**/*.rs', 'src/security/**/*.rs']). At least one entry recommended."
                },
                "focus": {
                    "type": "string",
                    "description": "Optional focus area: 'auth' / 'api' / 'data' / 'deps' / 'infra' / 'frontend'. Personas weight findings in that area higher."
                },
                "context_text": {
                    "type": "string",
                    "description": "Optional free-form context (recent diff, threat model notes, vendor advisories)."
                },
                "max_findings_per_persona": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20,
                    "description": "Cap on findings each red-team persona returns. Defaults to 8."
                },
                "slug": {
                    "type": "string",
                    "description": "Optional human-readable label appended to the report directory name."
                }
            },
            "required": ["scope"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        if let Err(error) = self.runtime.enforce(ToolOperation::Act, "security_audit") {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let raw_scope: Vec<String> = args
            .get("scope")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if raw_scope.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing 'scope' (at least one file path, directory, or glob required)".to_string()),
            });
        }
        let focus = args
            .get("focus")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();
        let context_text = args
            .get("context_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let max_findings = args
            .get("max_findings_per_persona")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 20) as usize;
        let slug_hint = args.get("slug").and_then(|v| v.as_str());

        let workspace = self.runtime.workspace_dir();
        let scope_samples = {
            let workspace = workspace.clone();
            let raw_scope = raw_scope.clone();
            match tokio::task::spawn_blocking(move || {
                collect_scope_samples(&workspace, &raw_scope, PER_FILE_BYTES, MAX_FILES)
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
        if scope_samples.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "scope produced 0 readable files; refine the paths/globs".to_string(),
                ),
            });
        }
        let scope_context = build_scope_context_snippet(&scope_samples, CONTEXT_CHAR_CAP);

        let personas = red_team_personas();
        let user_prompt = build_common_user_prompt(&focus, &context_text, &scope_context, max_findings);
        let tasks: Vec<PersonaTask> = personas
            .iter()
            .map(|p| PersonaTask {
                id: p.id.clone(),
                label: p.label.clone(),
                system_prompt: p.system_prompt.clone(),
                user_prompt: user_prompt.clone(),
            })
            .collect();

        let outcomes = fan_out_personas(&self.runtime, tasks, None, None).await;

        let mut findings: Vec<SecurityFinding> = Vec::new();
        let mut errors: Vec<(String, String)> = Vec::new();
        for outcome in outcomes {
            if let Some(err) = outcome.error.clone() {
                errors.push((outcome.label.clone(), err));
                continue;
            }
            let parsed = parse_security_response(&outcome.raw_response, &outcome.label);
            findings.extend(parsed);
        }
        let consolidated = consolidate_findings(findings);

        let stride_set: BTreeSet<String> = consolidated
            .iter()
            .filter_map(|f| {
                if f.stride.trim().is_empty() {
                    None
                } else {
                    Some(f.stride.trim().to_ascii_uppercase())
                }
            })
            .collect();
        let owasp_set: BTreeSet<String> = consolidated
            .iter()
            .filter_map(|f| {
                if f.owasp.trim().is_empty() {
                    None
                } else {
                    Some(f.owasp.trim().to_ascii_uppercase())
                }
            })
            .collect();

        let score = composite_score(&consolidated, owasp_set.len(), stride_set.len());

        let findings_md = render_findings_md(&focus, &consolidated, &errors);
        let owasp_md = render_coverage_md("OWASP Top-10", &OWASP_CATEGORIES, &owasp_set);
        let stride_md = render_coverage_md("STRIDE", &STRIDE_CATEGORIES, &stride_set);
        let report_dir = {
            let workspace = workspace.clone();
            let slug_hint = slug_hint.map(str::to_string);
            match tokio::task::spawn_blocking(move || -> Result<std::path::PathBuf, std::io::Error> {
                let report_dir = ensure_report_dir(&workspace, "security", slug_hint.as_deref())?;
                let _ = write_text(&report_dir.join("findings.md"), &findings_md);
                let _ = write_text(&report_dir.join("owasp-coverage.md"), &owasp_md);
                let _ = write_text(&report_dir.join("stride-coverage.md"), &stride_md);
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
            &focus,
            &consolidated,
            &owasp_set,
            &stride_set,
            score,
            &errors,
            &report_dir.join("findings.md"),
        );
        Ok(ToolResult {
            success: true,
            output: render_envelope("security", &console),
            error: None,
        })
    }
}

struct RedTeamPersona {
    id: String,
    label: String,
    system_prompt: String,
}

fn red_team_personas() -> Vec<RedTeamPersona> {
    vec![
        RedTeamPersona {
            id: "adversary".to_string(),
            label: "Security Adversary".to_string(),
            system_prompt: persona_system_prompt(
                "Security Adversary",
                "External attacker targeting injection vectors, authn/authz weaknesses, input validation, and data exposure.",
            ),
        },
        RedTeamPersona {
            id: "supply_chain".to_string(),
            label: "Supply Chain Attacker".to_string(),
            system_prompt: persona_system_prompt(
                "Supply Chain Attacker",
                "Compromised dependency, malicious build step, typosquatted package, postinstall script, leaked CI secret.",
            ),
        },
        RedTeamPersona {
            id: "insider".to_string(),
            label: "Malicious Insider".to_string(),
            system_prompt: persona_system_prompt(
                "Malicious Insider",
                "Authenticated low-privilege user attempting privilege escalation, data exfiltration, audit-log tampering, side-channel access.",
            ),
        },
        RedTeamPersona {
            id: "infra".to_string(),
            label: "Infrastructure Attacker".to_string(),
            system_prompt: persona_system_prompt(
                "Infrastructure Attacker",
                "Network-level attacker: TLS misconfig, exposed admin endpoints, leaked secrets, container/k8s misconfig, DoS.",
            ),
        },
    ]
}

fn persona_system_prompt(label: &str, focus: &str) -> String {
    format!(
        "You are the {label} on a red-team panel performing a STRIDE + OWASP Top-10 audit.\n\
         Your perspective: {focus}\n\
         Every finding MUST cite a concrete `path:line` location, classify the threat under \
         STRIDE (Spoofing / Tampering / Repudiation / Information Disclosure / Denial of \
         Service / Elevation of Privilege) AND OWASP Top-10 (A01 Broken Access Control, A02 \
         Cryptographic Failures, A03 Injection, A04 Insecure Design, A05 Security \
         Misconfiguration, A06 Vulnerable Components, A07 Identification & Authentication \
         Failures, A08 Software & Data Integrity Failures, A09 Logging & Monitoring \
         Failures, A10 Server-Side Request Forgery).\n\
         OUTPUT FORMAT: respond with ONE JSON object only, no markdown fences, no commentary:\n\
         {{\n  \"persona\": \"{label}\",\n  \"findings\": [\n    {{\n      \
           \"title\": \"<short imperative>\",\n      \"severity\": \"critical|high|medium|low|info\",\n      \
           \"stride\": \"S|T|R|I|D|E\",\n      \"owasp\": \"A01|A02|...|A10\",\n      \
           \"file_line\": \"<path:line>\",\n      \"evidence\": \"<1-3 sentences quoting code or behaviour>\",\n      \
           \"attack_scenario\": \"<concrete steps an attacker takes>\",\n      \
           \"remediation\": \"<actionable fix>\"\n    }}\n  ]\n}}\n\
         NEVER invent file paths or line numbers  -  if you cannot cite, leave `file_line` \
         empty. NEVER produce 'theoretical' findings without code evidence; if you cannot \
         build a concrete attack, omit the finding."
    )
}

fn build_common_user_prompt(
    focus: &str,
    context_text: &str,
    scope_context: &str,
    max_findings: usize,
) -> String {
    let extra = if context_text.trim().is_empty() {
        String::new()
    } else {
        format!("Orchestrator context:\n{context_text}\n\n")
    };
    format!(
        "Red-team audit task\n-------------------\n\
         Focus area: {focus}\n\
         Findings budget: at most {max_findings} findings.\n\n\
         {extra}\
         In-scope code samples (truncated):\n```\n{scope_context}\n```\n\n\
         Respond now with your JSON object."
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecurityFinding {
    title: String,
    severity: String,
    stride: String,
    owasp: String,
    file_line: String,
    evidence: String,
    attack_scenario: String,
    remediation: String,
    sources: Vec<String>,
}

fn parse_security_response(raw: &str, label: &str) -> Vec<SecurityFinding> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let block = extract_json_block(raw).unwrap_or(raw);
    let value: Value = match serde_json::from_str(block) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(arr) = value.get("findings").and_then(|v| v.as_array()) else {
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
            Some(SecurityFinding {
                title,
                severity,
                stride: item
                    .get("stride")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                owasp: item
                    .get("owasp")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                file_line: item
                    .get("file_line")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                evidence: item
                    .get("evidence")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                attack_scenario: item
                    .get("attack_scenario")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                remediation: item
                    .get("remediation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                sources: vec![label.to_string()],
            })
        })
        .collect()
}

fn consolidate_findings(findings: Vec<SecurityFinding>) -> Vec<SecurityFinding> {
    let mut grouped: HashMap<String, SecurityFinding> = HashMap::new();
    for finding in findings {
        let key = format!(
            "{}|{}|{}",
            finding.title.to_ascii_lowercase().trim(),
            finding.file_line.to_ascii_lowercase().trim(),
            finding.owasp.to_ascii_uppercase().trim()
        );
        grouped
            .entry(key)
            .and_modify(|existing| {
                if severity_rank(&finding.severity) > severity_rank(&existing.severity) {
                    existing.severity = finding.severity.clone();
                }
                if !finding.evidence.is_empty() && existing.evidence.is_empty() {
                    existing.evidence = finding.evidence.clone();
                }
                if !finding.attack_scenario.is_empty() && existing.attack_scenario.is_empty() {
                    existing.attack_scenario = finding.attack_scenario.clone();
                }
                if !finding.remediation.is_empty() && existing.remediation.is_empty() {
                    existing.remediation = finding.remediation.clone();
                }
                for src in &finding.sources {
                    if !existing.sources.contains(src) {
                        existing.sources.push(src.clone());
                    }
                }
            })
            .or_insert(finding);
    }
    let mut out: Vec<SecurityFinding> = grouped.into_values().collect();
    out.sort_by(|a, b| {
        severity_rank(&b.severity)
            .cmp(&severity_rank(&a.severity))
            .then_with(|| b.sources.len().cmp(&a.sources.len()))
            .then_with(|| a.title.cmp(&b.title))
    });
    out
}

const OWASP_CATEGORIES: [(&str, &str); 10] = [
    ("A01", "Broken Access Control"),
    ("A02", "Cryptographic Failures"),
    ("A03", "Injection"),
    ("A04", "Insecure Design"),
    ("A05", "Security Misconfiguration"),
    ("A06", "Vulnerable / Outdated Components"),
    ("A07", "Identification & Authentication Failures"),
    ("A08", "Software & Data Integrity Failures"),
    ("A09", "Logging & Monitoring Failures"),
    ("A10", "Server-Side Request Forgery"),
];

const STRIDE_CATEGORIES: [(&str, &str); 6] = [
    ("S", "Spoofing"),
    ("T", "Tampering"),
    ("R", "Repudiation"),
    ("I", "Information Disclosure"),
    ("D", "Denial of Service"),
    ("E", "Elevation of Privilege"),
];

fn composite_score(
    consolidated: &[SecurityFinding],
    owasp_covered: usize,
    stride_covered: usize,
) -> u32 {
    let owasp_part = (owasp_covered as u32 * 50) / 10;
    let stride_part = (stride_covered as u32 * 30) / 6;
    let finding_part = (consolidated.len() as u32).min(20);
    owasp_part + stride_part + finding_part
}

fn render_findings_md(
    focus: &str,
    findings: &[SecurityFinding],
    errors: &[(String, String)],
) -> String {
    let mut out = String::new();
    out.push_str("# Security Audit Findings\n\n");
    out.push_str(&format!("- Focus: {focus}\n"));
    out.push_str(&format!("- Total findings (after dedup): {}\n\n", findings.len()));
    if !errors.is_empty() {
        out.push_str("## Errored personas\n\n");
        for (label, err) in errors {
            out.push_str(&format!("- {label}: {err}\n"));
        }
        out.push('\n');
    }
    if findings.is_empty() {
        out.push_str("_No findings produced._\n");
        return out;
    }
    out.push_str("| # | Severity | OWASP | STRIDE | File:Line | Title | Sources |\n");
    out.push_str("|---|---|---|---|---|---|---|\n");
    for (idx, f) in findings.iter().enumerate() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | `{}` | {} | {} |\n",
            idx + 1,
            f.severity,
            f.owasp,
            f.stride,
            escape_md(&f.file_line),
            escape_md(&f.title),
            escape_md(&f.sources.join(", "))
        ));
    }
    out.push_str("\n## Details\n\n");
    for (idx, f) in findings.iter().enumerate() {
        out.push_str(&format!(
            "### {}. [{}] {}\n",
            idx + 1,
            f.severity,
            f.title
        ));
        if !f.file_line.is_empty() {
            out.push_str(&format!("- File: `{}`\n", f.file_line));
        }
        if !f.owasp.is_empty() {
            out.push_str(&format!("- OWASP: {}\n", f.owasp));
        }
        if !f.stride.is_empty() {
            out.push_str(&format!("- STRIDE: {}\n", f.stride));
        }
        if !f.evidence.is_empty() {
            out.push_str(&format!("- Evidence: {}\n", f.evidence));
        }
        if !f.attack_scenario.is_empty() {
            out.push_str(&format!("- Attack scenario: {}\n", f.attack_scenario));
        }
        if !f.remediation.is_empty() {
            out.push_str(&format!("- Remediation: {}\n", f.remediation));
        }
        if !f.sources.is_empty() {
            out.push_str(&format!("- Personas: {}\n", f.sources.join(", ")));
        }
        out.push('\n');
    }
    out
}

fn render_coverage_md(title: &str, categories: &[(&str, &str)], hits: &BTreeSet<String>) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {title} Coverage\n\n"));
    out.push_str(&format!(
        "Covered {}/{}\n\n",
        categories
            .iter()
            .filter(|(code, _)| hits.contains(&code.to_ascii_uppercase()))
            .count(),
        categories.len()
    ));
    out.push_str("| Code | Category | Covered |\n|---|---|---|\n");
    for (code, name) in categories {
        let covered = hits.contains(&code.to_ascii_uppercase());
        out.push_str(&format!(
            "| {code} | {name} | {} |\n",
            if covered { "yes" } else { "no" }
        ));
    }
    out
}

fn render_console_md(
    focus: &str,
    findings: &[SecurityFinding],
    owasp_set: &BTreeSet<String>,
    stride_set: &BTreeSet<String>,
    score: u32,
    errors: &[(String, String)],
    findings_path: &std::path::Path,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Security audit complete: {} findings (focus={focus})\n",
        findings.len()
    ));
    out.push_str(&format!(
        "Score: {score}/100 | OWASP coverage: {}/10 | STRIDE coverage: {}/6\n",
        owasp_set.len(),
        stride_set.len()
    ));
    if !errors.is_empty() {
        out.push_str(&format!(
            "Errored personas: {}\n",
            errors
                .iter()
                .map(|(label, _)| label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !findings.is_empty() {
        out.push_str("\nTop findings:\n");
        for (idx, f) in findings.iter().take(10).enumerate() {
            out.push_str(&format!(
                "{:>2}. [{}] {} ({} {})  -  `{}`\n",
                idx + 1,
                f.severity,
                escape_md(&f.title),
                f.owasp,
                f.stride,
                escape_md(&f.file_line)
            ));
        }
    }
    out.push_str(&format!(
        "\nFull report: `{}`\n",
        findings_path.display().to_string().replace('\\', "/")
    ));
    out
}

fn escape_md(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}
