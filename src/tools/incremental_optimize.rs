// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedChange {
    pub file: String,
    pub change_type: ChangeType,
    pub summary: String,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub verified: bool,
    pub verified_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Refactored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationCheckpoint {
    pub id: String,
    pub description: String,
    pub files: Vec<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSuggestion {
    pub id: String,
    pub target_file: String,
    pub category: SuggestionCategory,
    pub description: String,
    pub rationale: String,
    pub estimated_impact: ImpactLevel,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionCategory {
    Performance,
    Readability,
    Security,
    Maintainability,
    Testability,
    Documentation,
    TypeSafety,
    ErrorHandling,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
pub struct OptimizationState {
    pub checkpoints: Vec<OptimizationCheckpoint>,
    pub changes: Vec<TrackedChange>,
    pub suggestions: Vec<OptimizationSuggestion>,
    pub current_checkpoint_id: Option<String>,
    pub stats: OptimizationStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OptimizationStats {
    pub total_changes: usize,
    pub verified_changes: usize,
    pub suggestions_generated: usize,
    pub suggestions_applied: usize,
}

impl Default for OptimizationState {
    fn default() -> Self {
        Self {
            checkpoints: Vec::new(),
            changes: Vec::new(),
            suggestions: Vec::new(),
            current_checkpoint_id: None,
            stats: OptimizationStats::default(),
        }
    }
}

pub type OptimizationStateHandle = Arc<RwLock<OptimizationState>>;

pub struct IncrementalOptimizeTool {
    state: OptimizationStateHandle,
    workspace_root: Arc<RwLock<PathBuf>>,
}

impl IncrementalOptimizeTool {
    pub fn new(state: OptimizationStateHandle, workspace_root: Arc<RwLock<PathBuf>>) -> Self {
        Self {
            state,
            workspace_root,
        }
    }

    #[inline]
    fn project_workspace(&self) -> PathBuf {
        self.workspace_root.read().clone()
    }

    fn unix_now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn next_id(&self) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        format!("{}", COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    fn generate_suggestions_for_change(
        &self,
        change: &TrackedChange,
        content: &str,
    ) -> Vec<OptimizationSuggestion> {
        let mut suggestions = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        if change.change_type == ChangeType::Modified || change.change_type == ChangeType::Added {

            if content.contains("for ") && content.contains("for ") {
                let nested_for_count = content.matches("for ").count();
                if nested_for_count >= 2 {
                    suggestions.push(OptimizationSuggestion {
                        id: format!("s{}-perf-001", self.next_id()),
                        target_file: change.file.clone(),
                        category: SuggestionCategory::Performance,
                        description: format!(
                            "Potential nested loops detected ({nested_for_count}+ for loops). \
                             Consider if O(n^2) complexity is avoidable.",
                        ),
                        rationale: "Nested loops are a common source of O(n^2) performance issues."
                            .to_string(),
                        estimated_impact: ImpactLevel::Medium,
                        applied: false,
                    });
                }
            }

            if content.contains(".clone().clone()") || content.contains(".to_string().to_string()")
            {
                suggestions.push(OptimizationSuggestion {
                    id: format!("s{}-perf-002", self.next_id()),
                    target_file: change.file.clone(),
                    category: SuggestionCategory::Performance,
                    description: "Redundant clone/to_string chain detected.".to_string(),
                    rationale: "Chaining .clone() multiple times is wasteful.".to_string(),
                    estimated_impact: ImpactLevel::Low,
                    applied: false,
                });
            }

            if content.contains("unwrap()") && !content.contains('?') {
                suggestions.push(OptimizationSuggestion {
                    id: format!("s{}-err-001", self.next_id()),
                    target_file: change.file.clone(),
                    category: SuggestionCategory::ErrorHandling,
                    description: "`.unwrap()` found without `?` propagation. Consider using `?` or explicit error handling.".to_string(),
                    rationale: "`.unwrap()` panics on None/Err; explicit error handling is safer.".to_string(),
                    estimated_impact: ImpactLevel::Medium,
                    applied: false,
                });
            }

            if content.contains("TODO") || content.contains("FIXME") {
                suggestions.push(OptimizationSuggestion {
                    id: format!("s{}-doc-001", self.next_id()),
                    target_file: change.file.clone(),
                    category: SuggestionCategory::Documentation,
                    description: "TODO/FIXME comment detected.".to_string(),
                    rationale: "TODOs should have owner and deadline.".to_string(),
                    estimated_impact: ImpactLevel::Low,
                    applied: false,
                });
            }

            let secret_patterns = ["password", "api_key", "secret", "token", "credential"];
            let lower = content.to_lowercase();
            for pat in &secret_patterns {
                if lower.contains(*pat) && lower.contains('=') {
                    suggestions.push(OptimizationSuggestion {
                        id: format!("s{}-sec-001", self.next_id()),
                        target_file: change.file.clone(),
                        category: SuggestionCategory::Security,
                        description: format!(
                            "Possible hardcoded secret detected (pattern: '{pat}'). \
                             Move to environment variables or a secrets manager.",
                        ),
                        rationale: "Hardcoded secrets are a security risk.".to_string(),
                        estimated_impact: ImpactLevel::High,
                        applied: false,
                    });
                    break;
                }
            }

            let code_lines: Vec<&&str> = lines
                .iter()
                .filter(|l| {
                    !l.trim().is_empty()
                        && !l.trim().starts_with("//")
                        && !l.trim().starts_with("/*")
                        && !l.trim().starts_with('*')
                })
                .collect();
            if code_lines.len() > 100 {
                suggestions.push(OptimizationSuggestion {
                    id: format!("s{}-read-001", self.next_id()),
                    target_file: change.file.clone(),
                    category: SuggestionCategory::Readability,
                    description: format!(
                        "Function/file appears to have {} non-comment lines. Consider splitting.",
                        code_lines.len()
                    ),
                    rationale: "Large functions are harder to test and maintain.".to_string(),
                    estimated_impact: ImpactLevel::Medium,
                    applied: false,
                });
            }
        }

        suggestions
    }

    fn format_status(&self, state: &OptimizationState) -> String {
        let mut lines = Vec::new();
        lines.push("## Incremental Optimization Status\n".to_string());

        lines.push(format!("**Total changes**: {}", state.changes.len()));
        lines.push(format!(
            "**Verified**: {} / {}",
            state.stats.verified_changes,
            state.changes.len()
        ));
        lines.push(format!(
            "**Suggestions**: {} total, {} applied",
            state.stats.suggestions_generated, state.stats.suggestions_applied
        ));
        lines.push(String::new());

        if let Some(cp_id) = &state.current_checkpoint_id {
            lines.push(format!("**Current checkpoint**: `{}`", cp_id));
        } else {
            lines.push("**Current checkpoint**: none (call 'checkpoint' first)".to_string());
        }

        lines.push(String::new());
        lines.push("### Changes\n".to_string());

        if state.changes.is_empty() {
            lines.push("No changes tracked yet.".to_string());
        } else {
            for change in &state.changes {
                let verified_icon = if change.verified { "✅" } else { "⬜" };
                let change_type_str = match change.change_type {
                    ChangeType::Added => "A",
                    ChangeType::Modified => "M",
                    ChangeType::Deleted => "D",
                    ChangeType::Refactored => "R",
                };
                lines.push(format!(
                    "{verified_icon} [{change_type_str}] {summary} ({file})",
                    verified_icon = verified_icon,
                    change_type_str = change_type_str,
                    summary = change.summary,
                    file = change.file,
                ));
            }
        }

        lines.push(String::new());
        lines.push("### Suggestions\n".to_string());

        if state.suggestions.is_empty() {
            lines.push("No suggestions yet. Run 'suggest' after tracking changes.".to_string());
        } else {
            for sug in &state.suggestions {
                let applied_icon = if sug.applied { "✅" } else { "⬜" };
                let impact_icon = match sug.estimated_impact {
                    ImpactLevel::High => "[HIGH]",
                    ImpactLevel::Medium => "[MED]",
                    ImpactLevel::Low => "[low]",
                };
                lines.push(format!(
                    "{applied_icon} {impact_icon} [{category:?}] {description} ({file})\n  Rationale: {rationale}",
                    applied_icon = applied_icon,
                    impact_icon = impact_icon,
                    category = sug.category,
                    description = sug.description,
                    file = sug.target_file,
                    rationale = sug.rationale,
                ));
            }
        }

        lines.join("\n")
    }
}

#[async_trait]
impl Tool for IncrementalOptimizeTool {
    fn name(&self) -> &str {
        "incremental_optimize"
    }

    fn description(&self) -> &str {
        "Incremental optimization workflow: checkpoint state, track changes, \
         generate optimization suggestions, verify improvements, and report. \
         Actions: 'checkpoint' (record state), 'track' (record changes), \
         'suggest' (generate suggestions), 'verify' (run command + mark verified), \
         'report' (generate report), 'status' (show status). \
         This tool implements the Harness Layer 5: Capability Enhancement loop."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action: 'checkpoint' (record state), 'track' (record changes), 'suggest' (generate suggestions), 'verify' (mark verified), 'report' (generate report), 'status' (show status)",
                    "enum": ["checkpoint", "track", "suggest", "verify", "report", "status"]
                },
                "description": {
                    "type": "string",
                    "description": "Description for checkpoint/report (used with 'checkpoint' and 'report' actions)"
                },
                "file": {
                    "type": "string",
                    "description": "File path to track (used with 'track' action)"
                },
                "change_type": {
                    "type": "string",
                    "description": "Type of change (used with 'track' action)",
                    "enum": ["added", "modified", "deleted", "refactored"]
                },
                "summary": {
                    "type": "string",
                    "description": "Brief summary of the change (used with 'track' action)"
                },
                "lines_added": {
                    "type": "integer",
                    "description": "Number of lines added (used with 'track' action)",
                    "default": 0
                },
                "lines_removed": {
                    "type": "integer",
                    "description": "Number of lines removed (used with 'track' action)",
                    "default": 0
                },
                "change_id": {
                    "type": "integer",
                    "description": "Change ID to verify (used with 'verify' action)"
                },
                "suggestion_id": {
                    "type": "string",
                    "description": "Suggestion ID to mark as applied (used with 'verify' action)"
                },
                "command": {
                    "type": "string",
                    "description": "Verification command to run (used with 'verify' action). \
                                    Executes the command and reports pass/fail based on exit code. \
                                    Example: 'cargo test', 'npm test', 'pytest'"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        match action {
            "checkpoint" => {
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pre-change checkpoint")
                    .to_string();
                let checkpoint_id = format!("cp{}", self.next_id());
                let now = self.unix_now();

                let files: Vec<String> = self.list_code_files().await.unwrap_or_default();
                let files_count = files.len();

                let checkpoint = OptimizationCheckpoint {
                    id: checkpoint_id.clone(),
                    description: description.clone(),
                    files,
                    created_at: now,
                };

                {
                    let mut state = self.state.write();
                    state.current_checkpoint_id = Some(checkpoint_id.clone());
                    state.checkpoints.push(checkpoint);
                }

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Checkpoint `{}` created at Unix timestamp {}.\n\
                         Description: {}\n\
                         {} files recorded.",
                        checkpoint_id, now, description, files_count
                    ),
                    error: None,
                })
            }

            "track" => {
                let file = args
                    .get("file")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("'track' requires 'file' parameter"))?
                    .to_string();

                let change_type_str = args
                    .get("change_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("modified");

                let change_type = match change_type_str {
                    "added" => ChangeType::Added,
                    "modified" => ChangeType::Modified,
                    "deleted" => ChangeType::Deleted,
                    "refactored" => ChangeType::Refactored,
                    other => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!(
                                "Unknown change_type '{other}'. Use: added, modified, deleted, refactored."
                            )),
                        });
                    }
                };

                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(no summary)")
                    .to_string();

                let lines_added = args
                    .get("lines_added")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;

                let lines_removed = args
                    .get("lines_removed")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;

                let change_id = self.state.read().changes.len() + 1;

                let change = TrackedChange {
                    file: file.clone(),
                    change_type,
                    summary: summary.clone(),
                    lines_added,
                    lines_removed,
                    verified: false,
                    verified_at: None,
                };

                {
                    let mut state = self.state.write();
                    state.changes.push(change.clone());
                    state.stats.total_changes += 1;
                }

                Ok(ToolResult {
                    success: true,
                    output: format!(
                        "Change [{change_id}] tracked: {summary}\n\
                         File: {file}\n\
                         Type: {change_type:?}\n\
                         +{lines_added} -{lines_removed} lines\n\
                         \n\
                         Run 'suggest' to generate optimization recommendations.",
                        change_id = change_id,
                        summary = summary,
                        file = file,
                        change_type = change.change_type,
                        lines_added = lines_added,
                        lines_removed = lines_removed,
                    ),
                    error: None,
                })
            }

            "suggest" => {
                let changes: Vec<TrackedChange>;
                {
                    let state = self.state.read();
                    changes = state.changes.clone();
                }

                let ws = self.project_workspace();
                let file_paths: Vec<PathBuf> =
                    changes.iter().map(|c| ws.join(&c.file)).collect();
                let contents: Vec<Option<String>> = tokio::task::spawn_blocking(move || {
                    file_paths
                        .into_iter()
                        .map(|p| std::fs::read_to_string(&p).ok())
                        .collect()
                })
                .await?;

                let mut new_suggestions = Vec::new();
                {
                    let mut state = self.state.write();

                    for (change, content) in changes.iter().zip(contents.iter()) {
                        if let Some(content) = content {
                            let file_suggestions =
                                self.generate_suggestions_for_change(change, content);
                            for sug in file_suggestions {

                                if !state
                                    .suggestions
                                    .iter()
                                    .any(|s| s.description == sug.description)
                                {
                                    new_suggestions.push(sug.clone());
                                    state.suggestions.push(sug);
                                    state.stats.suggestions_generated += 1;
                                }
                            }
                        }
                    }
                }

                if new_suggestions.is_empty() {
                    return Ok(ToolResult {
                        success: true,
                        output: "No new optimization suggestions generated.\n\
                                 Either all suggestions are already generated, \
                                 or no tracked changes can be analyzed."
                            .to_string(),
                        error: None,
                    });
                }

                let mut output_lines = vec![format!(
                    "Generated {} new optimization suggestion(s):\n",
                    new_suggestions.len()
                )];
                for sug in &new_suggestions {
                    let impact_icon = match sug.estimated_impact {
                        ImpactLevel::High => "[HIGH] ",
                        ImpactLevel::Medium => "[MED]  ",
                        ImpactLevel::Low => "[low]  ",
                    };
                    output_lines.push(format!(
                        "{id} {impact}**[{category:?}]** {description}\n  → {rationale} ({file})\n",
                        id = sug.id,
                        impact = impact_icon,
                        category = sug.category,
                        description = sug.description,
                        rationale = sug.rationale,
                        file = sug.target_file,
                    ));
                }

                Ok(ToolResult {
                    success: true,
                    output: output_lines.join("\n"),
                    error: None,
                })
            }

            "verify" => {
                let mut verified_count = 0;
                let mut output_lines = Vec::new();
                let mut command_passed = false;

                let command = args.get("command").and_then(|v| v.as_str());
                if let Some(cmd) = command {

                    let forbidden_patterns = [
                        "--exec",
                        "--upload-pack",
                        "-c ",
                        "--no-verify",
                        "rm -rf /",
                        "mkfs.",
                        "> /dev/sd",
                        "dd if=",
                        "curl|sh",
                        "wget|sh",
                        "curl|bash",
                        "wget|bash",
                    ];
                    let cmd_lower = cmd.to_lowercase();
                    for pattern in &forbidden_patterns {
                        if cmd_lower.contains(&pattern.to_lowercase()) {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "Verification command blocked: contains forbidden pattern '{pattern}'"
                                )),
                            });
                        }
                    }

                    output_lines.push(format!("Running verification: {cmd}"));

                    let shell = if cfg!(windows) { "cmd" } else { "sh" };
                    let shell_arg = if cfg!(windows) { "/C" } else { "-c" };

                    let result = tokio::time::timeout(
                        std::time::Duration::from_secs(120),
                        crate::util::hidden_async_command(shell)
                            .args([shell_arg, cmd])
                            .current_dir(self.project_workspace())
                            .env_clear()
                            .envs(std::env::vars().filter(|(k, _)| {
                                matches!(
                                    k.as_str(),
                                    "PATH"
                                        | "HOME"
                                        | "USER"
                                        | "LANG"
                                        | "TERM"
                                        | "USERPROFILE"
                                        | "SYSTEMROOT"
                                        | "COMSPEC"
                                        | "TEMP"
                                        | "TMP"
                                        | "CARGO_HOME"
                                        | "RUSTUP_HOME"
                                )
                            }))
                            .output(),
                    )
                    .await;

                    let result = match result {
                        Ok(r) => r,
                        Err(_) => {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "Verification command '{cmd}' timed out after 120 seconds"
                                )),
                            });
                        }
                    };

                    match result {
                        Ok(output) => {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            let exit_code = output.status.code().unwrap_or(-1);
                            if output.status.success() {
                                output_lines.push(format!(
                                    "\u{2705} Verification PASSED (exit {exit_code})"
                                ));
                                command_passed = true;
                                if !stdout.is_empty() {
                                    output_lines.push(format!("Output:\n{}", stdout.trim()));
                                }
                            } else {
                                output_lines.push(format!(
                                    "\u{274c} Verification FAILED (exit {exit_code})"
                                ));
                                if !stdout.is_empty() {
                                    output_lines.push(format!("Stdout:\n{}", stdout.trim()));
                                }
                                if !stderr.is_empty() {
                                    output_lines.push(format!("Stderr:\n{}", stderr.trim()));
                                }
                                return Ok(ToolResult {
                                    success: false,
                                    output: output_lines.join("\n"),
                                    error: Some(format!(
                                        "Verification command '{cmd}' failed with exit code {exit_code}"
                                    )),
                                });
                            }
                        }
                        Err(e) => {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "Failed to run verification command '{cmd}': {e}"
                                )),
                            });
                        }
                    }
                }

                if let Some(change_id) = args.get("change_id").and_then(|v| v.as_u64()) {
                    let idx = (change_id - 1) as usize;
                    let found: bool;
                    let summary: Option<String>;
                    {
                        let mut state = self.state.write();
                        let change = state.changes.get_mut(idx);
                        found = change.is_some();
                        if let Some(change) = change {
                            change.verified = true;
                            change.verified_at = Some(self.unix_now());
                            summary = Some(change.summary.clone());
                        } else {
                            summary = None;
                        }
                    }
                    if found {
                        {
                            let mut state = self.state.write();
                            state.stats.verified_changes += 1;
                        }
                        verified_count += 1;
                        if let Some(s) = summary {
                            output_lines.push(format!("Change [{change_id}] verified: {s}"));
                        }
                    } else {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Change [{change_id}] not found.")),
                        });
                    }
                }

                if let Some(sug_id) = args.get("suggestion_id").and_then(|v| v.as_str()) {
                    let mut state = self.state.write();
                    if let Some(sug) = state.suggestions.iter_mut().find(|s| s.id == sug_id) {
                        sug.applied = true;
                        state.stats.suggestions_applied += 1;
                        output_lines.push(format!("Suggestion `{sug_id}` marked as applied."));
                    } else {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Suggestion `{sug_id}` not found.")),
                        });
                    }
                }

                if verified_count == 0 && !command_passed && output_lines.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(
                            "'verify' requires at least one of: 'change_id', 'suggestion_id', or 'command'.".to_string(),
                        ),
                    });
                }

                Ok(ToolResult {
                    success: true,
                    output: output_lines.join("\n"),
                    error: None,
                })
            }

            "report" => {
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Incremental optimization report")
                    .to_string();

                let state = self.state.read();
                let now = self.unix_now();

                let mut report = String::new();
                report.push_str(&format!("# {}\n\n", description));
                report.push_str(&format!("> Generated at Unix timestamp: {}\n\n", now));

                report.push_str("## Summary\n\n");
                report.push_str(&format!(
                    "- **Total changes tracked**: {}\n",
                    state.changes.len()
                ));
                report.push_str(&format!(
                    "- **Changes verified**: {} / {}\n",
                    state.stats.verified_changes,
                    state.changes.len()
                ));
                report.push_str(&format!(
                    "- **Suggestions generated**: {}\n",
                    state.stats.suggestions_generated
                ));
                report.push_str(&format!(
                    "- **Suggestions applied**: {} / {}\n\n",
                    state.stats.suggestions_applied, state.stats.suggestions_generated
                ));

                let verified_pct = if state.changes.is_empty() {
                    0.0
                } else {
                    (state.stats.verified_changes as f64 / state.changes.len() as f64) * 100.0
                };
                let applied_pct = if state.stats.suggestions_generated == 0 {
                    0.0
                } else {
                    (state.stats.suggestions_applied as f64
                        / state.stats.suggestions_generated as f64)
                        * 100.0
                };

                report.push_str(&format!("**Verification rate**: {:.1}%\n", verified_pct));
                report.push_str(&format!(
                    "**Suggestion adoption rate**: {:.1}%\n\n",
                    applied_pct
                ));

                report.push_str("## Checkpoints\n\n");
                if state.checkpoints.is_empty() {
                    report.push_str("No checkpoints recorded.\n\n");
                } else {
                    for cp in &state.checkpoints {
                        report.push_str(&format!(
                            "- `{}` (ts={}): {}\n  Files: {}\n",
                            cp.id,
                            cp.created_at,
                            cp.description,
                            cp.files.len()
                        ));
                    }
                    report.push('\n');
                }

                report.push_str("## Changes\n\n");
                for (i, change) in state.changes.iter().enumerate() {
                    let icon = if change.verified { "✅" } else { "⬜" };
                    let change_type_str = match change.change_type {
                        ChangeType::Added => "added",
                        ChangeType::Modified => "modified",
                        ChangeType::Deleted => "deleted",
                        ChangeType::Refactored => "refactored",
                    };
                    report.push_str(&format!(
                        "{icon} [{i}] {file} ({change_type_str}): {summary} (+{lines_added}, -{lines_removed})\n",
                        icon = icon,
                        i = i + 1,
                        file = change.file,
                        change_type_str = change_type_str,
                        summary = change.summary,
                        lines_added = change.lines_added,
                        lines_removed = change.lines_removed,
                    ));
                }
                report.push('\n');

                report.push_str("## Optimization Suggestions\n\n");
                if state.suggestions.is_empty() {
                    report.push_str("No suggestions generated.\n\n");
                } else {
                    for sug in &state.suggestions {
                        let applied_icon = if sug.applied { "✅" } else { "⬜" };
                        let impact_icon = match sug.estimated_impact {
                            ImpactLevel::High => "[HIGH]",
                            ImpactLevel::Medium => "[MED]",
                            ImpactLevel::Low => "[low]",
                        };
                        report.push_str(&format!(
                            "{applied_icon} {impact} [{category:?}] {description}\n  → {rationale}\n  → File: `{file}`\n\n",
                            applied_icon = applied_icon,
                            impact = impact_icon,
                            category = sug.category,
                            description = sug.description,
                            rationale = sug.rationale,
                            file = sug.target_file,
                        ));
                    }
                }

                report.push_str("---\n");
                report.push_str("*Generated by `incremental_optimize` tool (Harness Layer 5)*\n");

                Ok(ToolResult {
                    success: true,
                    output: report,
                    error: None,
                })
            }

            "status" => {
                let state = self.state.read();
                Ok(ToolResult {
                    success: true,
                    output: self.format_status(&state),
                    error: None,
                })
            }

            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{other}'. Use: 'checkpoint', 'track', 'suggest', 'verify', 'report', or 'status'."
                )),
            }),
        }
    }
}

impl IncrementalOptimizeTool {
    async fn list_code_files(&self) -> anyhow::Result<Vec<String>> {
        let ws = self.project_workspace();
        tokio::task::spawn_blocking(move || Self::list_code_files_blocking(&ws)).await?
    }

    fn list_code_files_blocking(ws: &std::path::Path) -> anyhow::Result<Vec<String>> {
        let extensions = [
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "toml", "yaml", "yml",
        ];
        let mut files = Vec::new();

        fn walk(
            dir: &std::path::Path,
            extensions: &[&str],
            workspace: &std::path::Path,
            files: &mut Vec<String>,
        ) -> anyhow::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !name.starts_with('.')
                            && name != "target"
                            && name != "node_modules"
                            && name != "__pycache__"
                            && name != "dist"
                            && name != "build"
                        {
                            walk(&path, extensions, workspace, files)?;
                        }
                    }
                } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if extensions.contains(&ext) {
                        if let Ok(rel) = path.strip_prefix(workspace) {
                            files.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
            Ok(())
        }

        walk(ws, &extensions, ws, &mut files)?;
        Ok(files)
    }
}
