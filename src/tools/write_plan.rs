// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::providers::Provider;
use crate::write_mode::executor::ApplyFnOutput;
use crate::write_mode::{LlmWritePlanner, PlanContext, WriteExecutor, WritePlanner};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct WritePlanTool;

impl WritePlanTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WritePlanTool {
    fn default() -> Self {
        Self::new()
    }
}

fn strip_code_fence(s: &str) -> String {
    let trimmed = s.trim();
    let body = trimmed
        .strip_prefix("```")
        .map(|rest| {
            rest.split_once('\n')
                .map(|(_lang, after)| after)
                .unwrap_or(rest)
        })
        .unwrap_or(trimmed);
    body.strip_suffix("```").unwrap_or(body).to_string()
}

#[async_trait]
impl Tool for WritePlanTool {
    fn name(&self) -> &str {
        "write_plan"
    }

    fn description(&self) -> &str {
        "Plan and execute a focused, multi-step code change for a single goal. Generates a \
         structured plan (read, edit, run, verify), applies each edit with automatic \
         verification and one refine-and-retry on failure, and rolls back edits that cannot be \
         verified. Use for self-contained edits that benefit from a verify-driven loop."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The change to make, described concretely. Mention the target file path when known."
                },
                "hint": {
                    "type": "string",
                    "description": "Optional additional guidance for the planner."
                }
            },
            "required": ["goal"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let goal = args
            .get("goal")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if goal.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("write_plan requires a non-empty `goal`".into()),
            });
        }
        let hint = args
            .get("hint")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty());

        let Some(svc) = crate::services::try_get_services() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("write_plan: runtime services unavailable".into()),
            });
        };
        let config = svc.config();
        let provider_name = config.default_provider.clone().unwrap_or_default();
        let model = config.default_model.clone().unwrap_or_default();
        if provider_name.is_empty() || model.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "write_plan: default_provider/default_model are not configured".into(),
                ),
            });
        }
        let workspace_root = config.workspace_dir.clone();

        let provider: Arc<dyn Provider> = {
            let options = crate::providers::provider_runtime_options_from_config(&config);
            match crate::providers::create_resilient_runtime_provider_async(
                provider_name,
                None,
                None,
                options,
            )
            .await
            {
                Ok(p) => Arc::from(p),
                Err(e) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("write_plan: failed to create provider: {e}")),
                    });
                }
            }
        };

        let ctx = PlanContext {
            goal: goal.to_string(),
            workspace_root,
            hint,
            allow_paths: Vec::new(),
        };

        let planner = LlmWritePlanner::new(provider.clone(), model.clone());
        let plan = match planner.plan(&ctx).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("write_plan: planning failed: {e}")),
                });
            }
        };

        let apply_provider = provider.clone();
        let apply_model = model.clone();
        let executor = WriteExecutor::new().with_apply_fn(
            move |source: &str, _path: &std::path::Path, instruction: Option<&str>, diff: Option<&str>| {
                let provider = apply_provider.clone();
                let model = apply_model.clone();
                let source = source.to_string();
                let instr = instruction
                    .map(str::to_string)
                    .or_else(|| diff.map(str::to_string))
                    .unwrap_or_default();
                Box::pin(async move {
                    let prompt = format!(
                        "Current file contents:\n----\n{source}\n----\n\nApply the following \
                         change and respond with ONLY the complete new file contents (no markdown \
                         fences, no commentary):\n{instr}"
                    );
                    let resp = provider
                        .chat_with_system(
                            Some(
                                "You are a precise code editor. Output only the full updated file \
                                 contents.",
                            ),
                            &prompt,
                            &model,
                            0.0,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(strip_code_fence(&resp))
                }) as ApplyFnOutput
            },
        );

        match executor.execute(&ctx, &plan).await {
            Ok((outcomes, verify)) => {
                let mut lines = vec![format!("{} | {:?}", plan.summary(), verify)];
                for o in &outcomes {
                    lines.push(format!("  - {}: {}", o.label, o.summary));
                }
                Ok(ToolResult {
                    success: matches!(verify, crate::write_mode::VerifyOutcome::Passed
                        | crate::write_mode::VerifyOutcome::Absent),
                    output: lines.join("\n"),
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("write_plan: execution failed: {e}")),
            }),
        }
    }
}
