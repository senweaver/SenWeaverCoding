// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Planners turn a [`PlanContext`] into a concrete [`WritePlan`].
//!
//! Two implementations ship:
//!
//! * [`LlmWritePlanner`] ??production planner that prompts an LLM
//!   provider and parses the JSON response.
//! * [`HeuristicPlanner`] ??deterministic fallback (and default test
//!   double).  Inspects the goal string for a single target path and
//!   emits a 4-step plan: `read_file ??apply_diff ??run_command ??//!   verify`.  Useful when the LLM is unavailable and for test
//!   fixtures that need a known-shape plan.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

use super::MAX_PLAN_STEPS;
use super::prompts::{PLAN_SYSTEM_PROMPT, build_plan_user_prompt};
use super::types::{PlanContext, WritePlan, WriteStep};
use crate::observability::session_write_mode_metrics;

#[async_trait]
pub trait WritePlanner: Send + Sync + std::fmt::Debug {
    async fn plan(&self, ctx: &PlanContext) -> Result<WritePlan, PlannerError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error("llm call failed: {0}")]
    Llm(String),
    #[error("plan json malformed: {0}")]
    Malformed(String),
    #[error("plan exceeds {MAX_PLAN_STEPS} steps (got {0})")]
    TooManySteps(usize),
    #[error("plan terminates without a verify step")]
    MissingVerify,
}

#[derive(Debug, Clone, Default)]
pub struct HeuristicPlanner;

#[async_trait]
impl WritePlanner for HeuristicPlanner {
    async fn plan(&self, ctx: &PlanContext) -> Result<WritePlan, PlannerError> {
        session_write_mode_metrics::incr_write_mode_plan();
        let target = first_path_like(&ctx.goal).unwrap_or_else(|| PathBuf::from("src/lib.rs"));
        let steps = vec![
            WriteStep::ReadFile {
                path: target.clone(),
            },
            WriteStep::ApplyDiff {
                path: target.clone(),
                instruction: Some(ctx.goal.clone()),
                diff: None,
            },
            WriteStep::RunCommand {
                command: "cargo check --lib".into(),
                cwd: Some(ctx.workspace_root.clone()),
            },
            WriteStep::Verify {
                expect_contains: vec!["Finished".into()],
            },
        ];
        session_write_mode_metrics::add_write_mode_steps(steps.len() as u64);
        validate_plan(&ctx.goal, steps).map(|plan| {
            session_write_mode_metrics::incr_write_mode_plan_ok();
            plan
        })
    }
}

pub struct LlmWritePlanner {
    provider: Arc<dyn crate::providers::Provider>,
    model: String,
    temperature: f64,
}

impl std::fmt::Debug for LlmWritePlanner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmWritePlanner")
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .finish_non_exhaustive()
    }
}

impl LlmWritePlanner {
    #[must_use]
    pub fn new(provider: Arc<dyn crate::providers::Provider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature: 0.0,
        }
    }

    #[must_use]
    pub fn with_temperature(mut self, t: f64) -> Self {
        self.temperature = t;
        self
    }
}

#[async_trait]
impl WritePlanner for LlmWritePlanner {
    async fn plan(&self, ctx: &PlanContext) -> Result<WritePlan, PlannerError> {
        session_write_mode_metrics::incr_write_mode_plan();
        let user = build_plan_user_prompt(ctx);
        let response = self
            .provider
            .chat_with_system(
                Some(PLAN_SYSTEM_PROMPT),
                &user,
                &self.model,
                self.temperature,
            )
            .await
            .map_err(|e| PlannerError::Llm(e.to_string()))?;
        let json = strip_markdown_fence(&response);
        let parsed: WritePlan =
            serde_json::from_str(json).map_err(|e| PlannerError::Malformed(e.to_string()))?;
        let out = validate_plan(&ctx.goal, parsed.steps)?;
        session_write_mode_metrics::add_write_mode_steps(out.steps.len() as u64);
        session_write_mode_metrics::incr_write_mode_plan_ok();
        Ok(out)
    }
}

fn strip_markdown_fence(s: &str) -> &str {
    let trimmed = s.trim();
    let without_lead = trimmed
        .strip_prefix("```json\n")
        .or_else(|| trimmed.strip_prefix("```JSON\n"))
        .or_else(|| trimmed.strip_prefix("```\n"))
        .unwrap_or(trimmed);
    without_lead.strip_suffix("\n```").unwrap_or(without_lead)
}

fn validate_plan(goal: &str, mut steps: Vec<WriteStep>) -> Result<WritePlan, PlannerError> {
    if steps.is_empty() {
        return Err(PlannerError::MissingVerify);
    }
    if steps.len() > MAX_PLAN_STEPS {
        return Err(PlannerError::TooManySteps(steps.len()));
    }
    match steps.last() {
        Some(WriteStep::Verify { .. }) => {}
        _ => {

            steps.push(WriteStep::Verify {
                expect_contains: vec![],
            });
        }
    }
    Ok(WritePlan::new(goal.to_string(), steps))
}

fn first_path_like(goal: &str) -> Option<PathBuf> {
    const SUFFIXES: [&str; 9] = [
        ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go", ".toml", ".md",
    ];
    goal.split(|c: char| c.is_whitespace() || matches!(c, '`' | ',' | '(' | ')'))
        .find(|tok| {
            (tok.contains('/') || tok.contains('\\')) && SUFFIXES.iter().any(|s| tok.ends_with(s))
        })
        .map(PathBuf::from)
}
