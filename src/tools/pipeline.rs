// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::PipelineConfig;
use crate::tools::traits::{Tool, ToolResult};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, thiserror::Error)]
pub enum PipelineError {
    #[error("Unknown tool '{0}' is not on the allowed list")]
    UnknownTool(String),
    #[error("Pipeline exceeds maximum of {0} steps")]
    TooManySteps(usize),
    #[error("Invalid template reference: {0}")]
    InvalidTemplate(String),
    #[error("Step {index} ({tool}) failed: {message}")]
    StepFailed {
        index: usize,
        tool: String,
        message: String,
    },
}

impl crate::error::ErrorClassification for PipelineError {
    fn category(&self) -> crate::error::ErrorCategory {
        use crate::error::ErrorCategory;
        match self {
            PipelineError::UnknownTool(_) => ErrorCategory::NotFound,
            PipelineError::TooManySteps(_) | PipelineError::InvalidTemplate(_) => {
                ErrorCategory::Validation
            }
            PipelineError::StepFailed { message, .. } => {
                let lower = message.to_lowercase();
                if lower.contains("timeout") {
                    ErrorCategory::Timeout
                } else if lower.contains("cancel") {
                    ErrorCategory::Cancelled
                } else {
                    ErrorCategory::Internal
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub tool: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRequest {
    pub steps: Vec<PipelineStep>,
    #[serde(default)]
    pub parallel: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub index: usize,
    pub tool: String,
    pub success: bool,
    pub output: String,
}

pub struct PipelineTool {
    config: PipelineConfig,
    tools: Vec<Arc<dyn Tool>>,
    allowed_set: HashSet<String>,
}

impl PipelineTool {
    pub fn new(config: PipelineConfig, tools: Vec<Arc<dyn Tool>>) -> Self {
        let allowed_set: HashSet<String> = config.allowed_tools.iter().cloned().collect();
        Self {
            config,
            tools,
            allowed_set,
        }
    }

    fn find_tool(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    fn validate(&self, request: &PipelineRequest) -> std::result::Result<(), PipelineError> {
        if request.steps.len() > self.config.max_steps {
            return Err(PipelineError::TooManySteps(self.config.max_steps));
        }

        for step in &request.steps {
            if !self.allowed_set.contains(&step.tool) {
                return Err(PipelineError::UnknownTool(step.tool.clone()));
            }
        }

        Ok(())
    }

    async fn execute_sequential(
        &self,
        steps: &[PipelineStep],
    ) -> std::result::Result<Vec<StepResult>, PipelineError> {
        let mut results: Vec<StepResult> = Vec::with_capacity(steps.len());

        for (i, step) in steps.iter().enumerate() {
            let tool = self
                .find_tool(&step.tool)
                .ok_or_else(|| PipelineError::UnknownTool(step.tool.clone()))?;

            let interpolated_args = interpolate_args(&step.args, &results);

            let tool_result =
                tool.execute(interpolated_args)
                    .await
                    .map_err(|e| PipelineError::StepFailed {
                        index: i,
                        tool: step.tool.clone(),
                        message: e.to_string(),
                    })?;

            if !tool_result.success {
                return Err(PipelineError::StepFailed {
                    index: i,
                    tool: step.tool.clone(),
                    message: tool_result
                        .error
                        .unwrap_or_else(|| tool_result.output.clone()),
                });
            }

            results.push(StepResult {
                index: i,
                tool: step.tool.clone(),
                success: true,
                output: tool_result.output,
            });
        }

        Ok(results)
    }

    async fn execute_parallel(
        &self,
        steps: &[PipelineStep],
    ) -> std::result::Result<Vec<StepResult>, PipelineError> {
        use tokio::task::JoinSet;

        let mut join_set = JoinSet::new();

        for (i, step) in steps.iter().enumerate() {
            let tool = self
                .find_tool(&step.tool)
                .ok_or_else(|| PipelineError::UnknownTool(step.tool.clone()))?;

            let tool_name = step.tool.clone();
            let args = step.args.clone();

            let tool_arc = self.tools.iter().find(|t| t.name() == tool.name()).cloned();

            if let Some(tool_arc) = tool_arc {
                join_set.spawn(async move {
                    let result = tool_arc.execute(args).await;
                    (i, tool_name, result)
                });
            }
        }

        let mut results: Vec<StepResult> = Vec::with_capacity(steps.len());

        while let Some(join_result) = join_set.join_next().await {
            let (index, tool_name, tool_result) =
                join_result.map_err(|e| PipelineError::StepFailed {
                    index: 0,
                    tool: "unknown".to_string(),
                    message: format!("Task join error: {e}"),
                })?;

            let tool_result = tool_result.map_err(|e| PipelineError::StepFailed {
                index,
                tool: tool_name.clone(),
                message: e.to_string(),
            })?;

            if !tool_result.success {
                return Err(PipelineError::StepFailed {
                    index,
                    tool: tool_name,
                    message: tool_result
                        .error
                        .unwrap_or_else(|| tool_result.output.clone()),
                });
            }

            results.push(StepResult {
                index,
                tool: tool_name,
                success: true,
                output: tool_result.output,
            });
        }

        results.sort_by_key(|r| r.index);
        Ok(results)
    }
}

#[async_trait]
impl Tool for PipelineTool {
    fn name(&self) -> &str {
        "execute_pipeline"
    }

    fn description(&self) -> &str {
        "Execute a multi-step tool pipeline in a single call. Steps run sequentially by default \
         with result interpolation (use {{step[N].result}} to reference prior outputs), \
         or in parallel when 'parallel: true' is set."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "description": "Ordered list of tool invocations",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "description": "Name of the tool to invoke"
                            },
                            "args": {
                                "type": "object",
                                "description": "Arguments to pass to the tool. Use {{step[N].result}} to interpolate prior step outputs."
                            }
                        },
                        "required": ["tool", "args"]
                    }
                },
                "parallel": {
                    "type": "boolean",
                    "description": "Run steps in parallel (no interpolation). Default: false",
                    "default": false
                }
            },
            "required": ["steps"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let request: PipelineRequest = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("Invalid pipeline request: {e}"))?;

        if let Err(e) = self.validate(&request) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            });
        }

        let results = if request.parallel {
            self.execute_parallel(&request.steps).await
        } else {
            self.execute_sequential(&request.steps).await
        };

        match results {
            Ok(step_results) => {
                let output = serde_json::to_string_pretty(&step_results)
                    .unwrap_or_else(|_| "Pipeline completed".to_string());
                Ok(ToolResult {
                    success: true,
                    output,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}

pub fn interpolate_args(
    args: &serde_json::Value,
    prior_results: &[StepResult],
) -> serde_json::Value {
    match args {
        serde_json::Value::String(s) => {
            let interpolated = interpolate_string(s, prior_results);
            serde_json::Value::String(interpolated)
        }
        serde_json::Value::Object(map) => {
            let new_map: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), interpolate_args(v, prior_results)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            let new_arr: Vec<serde_json::Value> = arr
                .iter()
                .map(|v| interpolate_args(v, prior_results))
                .collect();
            serde_json::Value::Array(new_arr)
        }
        other => other.clone(),
    }
}

fn interpolate_string(s: &str, prior_results: &[StepResult]) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c == '{' {
            if let Some(&(_, '{')) = chars.peek() {

                let rest = &s[i..];
                if let Some(end) = find_template_end(rest) {
                    let template = &rest[2..end];
                    if let Some(value) = resolve_template(template, prior_results) {

                        result.push_str(&value.replace("{{", ""));

                        let skip_to = i + end + 2;
                        while chars.peek().is_some_and(|&(idx, _)| idx < skip_to) {
                            chars.next();
                        }
                        continue;
                    }
                }
            }
        }
        result.push(c);
    }

    result
}

fn find_template_end(s: &str) -> Option<usize> {
    s[2..].find("}}").map(|pos| pos + 2)
}

fn resolve_template(template: &str, prior_results: &[StepResult]) -> Option<String> {
    let template = template.trim();
    if !template.starts_with("step[") || !template.ends_with(".result") {
        return None;
    }

    let bracket_end = template.find(']')?;
    let index_str = &template[5..bracket_end];
    let index: usize = index_str.parse().ok()?;

    prior_results
        .iter()
        .find(|r| r.index == index)
        .map(|r| r.output.clone())
}
