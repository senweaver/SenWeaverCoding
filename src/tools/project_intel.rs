// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(feature = "tool-reports")]
use super::report_templates;
use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
#[cfg(feature = "tool-reports")]
use std::collections::HashMap;
use std::fmt::Write as _;

pub struct ProjectIntelTool {
    default_language: String,
    risk_sensitivity: RiskSensitivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskSensitivity {
    Low,
    Medium,
    High,
}

impl RiskSensitivity {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "low" => Self::Low,
            "high" => Self::High,
            _ => Self::Medium,
        }
    }

    fn threshold_factor(self) -> f64 {
        match self {
            Self::Low => 1.5,
            Self::Medium => 1.0,
            Self::High => 0.5,
        }
    }
}

impl ProjectIntelTool {
    pub fn new(default_language: String, risk_sensitivity: String) -> Self {
        Self {
            default_language,
            risk_sensitivity: RiskSensitivity::from_str(&risk_sensitivity),
        }
    }

    fn execute_status_report(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let project_name = args
            .get("project_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing required 'project_name' for status_report"))?;
        let period = args
            .get("period")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing required 'period' for status_report"))?;
        let lang = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_language);
        let git_log = args
            .get("git_log")
            .and_then(|v| v.as_str())
            .unwrap_or("No git data provided");
        let jira_summary = args
            .get("jira_summary")
            .and_then(|v| v.as_str())
            .unwrap_or("No Jira data provided");
        let notes = args.get("notes").and_then(|v| v.as_str()).unwrap_or("");

        #[cfg(feature = "tool-reports")]
        let rendered = {
            let tpl = report_templates::weekly_status_template(lang);
            let mut vars = HashMap::new();
            vars.insert("project_name".into(), project_name.to_string());
            vars.insert("period".into(), period.to_string());
            vars.insert("completed".into(), git_log.to_string());
            vars.insert("in_progress".into(), jira_summary.to_string());
            vars.insert("blocked".into(), notes.to_string());
            vars.insert("next_steps".into(), "To be determined".into());
            tpl.render(&vars)
        };
        #[cfg(not(feature = "tool-reports"))]
        let rendered = {
            let _ = lang;
            format!(
                "## Weekly Status Report: {project_name}\n\n**Period:** {period}\n\n\
                ## Completed\n\n{git_log}\n\n\
                ## In Progress\n\n{jira_summary}\n\n\
                ## Blocked\n\n{blocked}\n\n\
                ## Next Steps\n\nTo be determined",
                blocked = if notes.is_empty() { "None" } else { notes }
            )
        };
        Ok(ToolResult {
            success: true,
            output: rendered,
            error: None,
        })
    }

    fn execute_risk_scan(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let deadlines = args
            .get("deadlines")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let velocity = args
            .get("velocity")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let blockers = args
            .get("blockers")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let lang = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_language);

        let mut risks = Vec::new();

        let factor = self.risk_sensitivity.threshold_factor();

        if !blockers.is_empty() {
            let blocker_count = blockers.lines().filter(|l| !l.trim().is_empty()).count();
            let severity = if (blocker_count as f64) > 3.0 * factor {
                "critical"
            } else if (blocker_count as f64) > 1.0 * factor {
                "high"
            } else {
                "medium"
            };
            risks.push(RiskItem {
                title: "Active blockers detected".into(),
                severity: severity.into(),
                detail: format!("{blocker_count} blocker(s) identified"),
                mitigation: "Escalate blockers, assign owners, set resolution deadlines".into(),
            });
        }

        if deadlines.to_lowercase().contains("overdue")
            || deadlines.to_lowercase().contains("missed")
        {
            risks.push(RiskItem {
                title: "Deadline risk".into(),
                severity: "high".into(),
                detail: "Overdue or missed deadlines detected in project context".into(),
                mitigation: "Re-prioritize scope, negotiate timeline, add resources".into(),
            });
        }

        if velocity.to_lowercase().contains("declining") || velocity.to_lowercase().contains("slow")
        {
            risks.push(RiskItem {
                title: "Velocity degradation".into(),
                severity: "medium".into(),
                detail: "Team velocity is declining or below expectations".into(),
                mitigation: "Identify bottlenecks, reduce WIP, address technical debt".into(),
            });
        }

        if risks.is_empty() {
            risks.push(RiskItem {
                title: "No significant risks detected".into(),
                severity: "low".into(),
                detail: "Current project signals within normal parameters".into(),
                mitigation: "Continue monitoring".into(),
            });
        }

        let risks_text = risks
            .iter()
            .map(|r| {
                format!(
                    "- [{}] {}: {}",
                    r.severity.to_uppercase(),
                    r.title,
                    r.detail
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mitigations_text = risks
            .iter()
            .map(|r| format!("- {}: {}", r.title, r.mitigation))
            .collect::<Vec<_>>()
            .join("\n");
        let project_name_str = args
            .get("project_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        #[cfg(feature = "tool-reports")]
        let output = {
            let tpl = report_templates::risk_register_template(lang);
            let mut vars = HashMap::new();
            vars.insert("project_name".into(), project_name_str);
            vars.insert("risks".into(), risks_text);
            vars.insert("mitigations".into(), mitigations_text);
            tpl.render(&vars)
        };
        #[cfg(not(feature = "tool-reports"))]
        let output = {
            let _ = lang;
            format!(
                "## Risk Register: {project_name_str}\n\n\
                ### Identified Risks\n\n{risks_text}\n\n\
                ### Mitigations\n\n{mitigations_text}"
            )
        };
        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }

    fn execute_draft_update(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let project_name = args
            .get("project_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing required 'project_name' for draft_update"))?;
        let audience = args
            .get("audience")
            .and_then(|v| v.as_str())
            .unwrap_or("client");
        let tone = args
            .get("tone")
            .and_then(|v| v.as_str())
            .unwrap_or("formal");
        let highlights = args
            .get("highlights")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing required 'highlights' for draft_update"))?;
        let concerns = args.get("concerns").and_then(|v| v.as_str()).unwrap_or("");

        let greeting = match (audience, tone) {
            ("client", "casual") => "Hi there,".to_string(),
            ("client", _) => "Dear valued partner,".to_string(),
            ("internal", "casual") => "Hey team,".to_string(),
            ("internal", _) => "Dear team,".to_string(),
            (_, "casual") => "Hi,".to_string(),
            _ => "Dear reader,".to_string(),
        };

        let closing = match tone {
            "casual" => "Cheers",
            _ => "Best regards",
        };

        let mut body = format!(
            "{greeting}\n\nHere is an update on {project_name}.\n\n**Highlights:**\n{highlights}"
        );
        if !concerns.is_empty() {
            let _ = write!(body, "\n\n**Items requiring attention:**\n{concerns}");
        }
        let _ = write!(
            body,
            "\n\nPlease do not hesitate to reach out with any questions.\n\n{closing}"
        );

        Ok(ToolResult {
            success: true,
            output: body,
            error: None,
        })
    }

    fn execute_sprint_summary(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let sprint_dates = args
            .get("sprint_dates")
            .and_then(|v| v.as_str())
            .unwrap_or("current sprint");
        let completed = args
            .get("completed")
            .and_then(|v| v.as_str())
            .unwrap_or("None specified");
        let in_progress = args
            .get("in_progress")
            .and_then(|v| v.as_str())
            .unwrap_or("None specified");
        let blocked = args
            .get("blocked")
            .and_then(|v| v.as_str())
            .unwrap_or("None");
        let velocity = args
            .get("velocity")
            .and_then(|v| v.as_str())
            .unwrap_or("Not calculated");
        let lang = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_language);

        #[cfg(feature = "tool-reports")]
        let rendered = {
            let tpl = report_templates::sprint_review_template(lang);
            let mut vars = HashMap::new();
            vars.insert("sprint_dates".into(), sprint_dates.to_string());
            vars.insert("completed".into(), completed.to_string());
            vars.insert("in_progress".into(), in_progress.to_string());
            vars.insert("blocked".into(), blocked.to_string());
            vars.insert("velocity".into(), velocity.to_string());
            tpl.render(&vars)
        };
        #[cfg(not(feature = "tool-reports"))]
        let rendered = {
            let _ = lang;
            format!(
                "## Sprint Review: {sprint_dates}\n\n\
                ## Completed\n\n{completed}\n\n\
                ## In Progress\n\n{in_progress}\n\n\
                ## Blocked\n\n{blocked}\n\n\
                ## Velocity\n\n{velocity}"
            )
        };
        Ok(ToolResult {
            success: true,
            output: rendered,
            error: None,
        })
    }

    fn execute_effort_estimate(&self, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let tasks = args.get("tasks").and_then(|v| v.as_str()).unwrap_or("");

        if tasks.trim().is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("No task descriptions provided".into()),
            });
        }

        let mut estimates = Vec::new();
        for line in tasks.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (size, rationale) = estimate_task_effort(line);
            estimates.push(format!("- **{size}** | {line}\n  Rationale: {rationale}"));
        }

        let output = format!(
            "## Effort Estimates\n\n{}\n\n_Sizes: XS (<2h), S (2-4h), M (4-8h), L (1-3d), XL (3-5d), XXL (>5d)_",
            estimates.join("\n")
        );

        Ok(ToolResult {
            success: true,
            output,
            error: None,
        })
    }
}

struct RiskItem {
    title: String,
    severity: String,
    detail: String,
    mitigation: String,
}

fn estimate_task_effort(description: &str) -> (&'static str, &'static str) {
    let lower = description.to_lowercase();
    let word_count = description.split_whitespace().count();

    let complexity_signals = [
        "refactor",
        "rewrite",
        "migrate",
        "redesign",
        "architecture",
        "infrastructure",
    ];
    let medium_signals = [
        "implement",
        "create",
        "build",
        "integrate",
        "add feature",
        "new module",
    ];
    let small_signals = [
        "fix", "update", "tweak", "adjust", "rename", "typo", "bump", "config",
    ];

    if complexity_signals.iter().any(|s| lower.contains(s)) {
        if word_count > 15 {
            return (
                "XXL",
                "Large-scope structural change with extensive description",
            );
        }
        return ("XL", "Structural change requiring significant effort");
    }

    if medium_signals.iter().any(|s| lower.contains(s)) {
        if word_count > 12 {
            return ("L", "Feature implementation with detailed requirements");
        }
        return ("M", "Standard feature implementation");
    }

    if small_signals.iter().any(|s| lower.contains(s)) {
        if word_count > 10 {
            return ("S", "Small change with additional context");
        }
        return ("XS", "Minor targeted change");
    }

    if word_count > 20 {
        ("L", "Complex task inferred from detailed description")
    } else if word_count > 10 {
        ("M", "Moderate task inferred from description length")
    } else {
        ("S", "Simple task inferred from brief description")
    }
}

#[async_trait]
impl Tool for ProjectIntelTool {
    fn name(&self) -> &str {
        "project_intel"
    }

    fn description(&self) -> &str {
        "Project delivery intelligence: generate status reports, detect risks, draft client updates, summarize sprints, and estimate effort. Read-only analysis tool."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status_report", "risk_scan", "draft_update", "sprint_summary", "effort_estimate"],
                    "description": "The analysis action to perform"
                },
                "project_name": {
                    "type": "string",
                    "description": "Project name (for status_report, risk_scan, draft_update)"
                },
                "period": {
                    "type": "string",
                    "description": "Reporting period: week, sprint, or month (for status_report)"
                },
                "language": {
                    "type": "string",
                    "description": "Report language: en, de, fr, it (default from config)"
                },
                "git_log": {
                    "type": "string",
                    "description": "Git log summary text (for status_report)"
                },
                "jira_summary": {
                    "type": "string",
                    "description": "Jira/issue tracker summary (for status_report)"
                },
                "notes": {
                    "type": "string",
                    "description": "Additional notes or context"
                },
                "deadlines": {
                    "type": "string",
                    "description": "Deadline information (for risk_scan)"
                },
                "velocity": {
                    "type": "string",
                    "description": "Team velocity data (for risk_scan, sprint_summary)"
                },
                "blockers": {
                    "type": "string",
                    "description": "Current blockers (for risk_scan)"
                },
                "audience": {
                    "type": "string",
                    "enum": ["client", "internal"],
                    "description": "Target audience (for draft_update)"
                },
                "tone": {
                    "type": "string",
                    "enum": ["formal", "casual"],
                    "description": "Communication tone (for draft_update)"
                },
                "highlights": {
                    "type": "string",
                    "description": "Key highlights for the update (for draft_update)"
                },
                "concerns": {
                    "type": "string",
                    "description": "Items requiring attention (for draft_update)"
                },
                "sprint_dates": {
                    "type": "string",
                    "description": "Sprint date range (for sprint_summary)"
                },
                "completed": {
                    "type": "string",
                    "description": "Completed items (for sprint_summary)"
                },
                "in_progress": {
                    "type": "string",
                    "description": "In-progress items (for sprint_summary)"
                },
                "blocked": {
                    "type": "string",
                    "description": "Blocked items (for sprint_summary)"
                },
                "tasks": {
                    "type": "string",
                    "description": "Task descriptions, one per line (for effort_estimate)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required 'action' parameter"))?;

        match action {
            "status_report" => self.execute_status_report(&args),
            "risk_scan" => self.execute_risk_scan(&args),
            "draft_update" => self.execute_draft_update(&args),
            "sprint_summary" => self.execute_sprint_summary(&args),
            "effort_estimate" => self.execute_effort_estimate(&args),
            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{other}'. Valid actions: status_report, risk_scan, draft_update, sprint_summary, effort_estimate"
                )),
            }),
        }
    }
}
