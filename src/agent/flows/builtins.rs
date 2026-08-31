// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::code_edit_plan::{
    PlanDependencyGraph, PlanStepJson, auto_expand_with_symbol_graph, degraded_catch_all_step,
    render_planner_prompt, render_planner_retry_prompt, step_from_plan, validate_planner_response,
};
use super::plan_exec_verify::{LayeredPlan, PlanExecVerifyFlow, PlanExecVerifyOptions};
use super::traits::{
    AgentHandle, Artifact, ExecOutcome, Executor, Flow, FlowContext, FlowError, FlowOutcome,
    Planner, Step, TranscriptEntry, VerificationVerdict, Verifier,
};
use crate::agent::self_assess::critic::CriticContext;
use crate::apply_model::edit_op::{EditBatch, EditOp, EditOrigin};
use crate::apply_model::ops_applier::OpsApplier;
use crate::code_intel::outline::extract_outline;
use crate::inline_edit::runtime_config::{CodeEditSection, RuntimeConfig};

pub struct CodeEditFlow {
    pub language: String,
    pub options: PlanExecVerifyOptions,
    pub code_edit_cfg: CodeEditSection,
    pub critic: Option<CriticContext>,
}

impl Default for CodeEditFlow {
    fn default() -> Self {
        let cfg = CodeEditSection::default();
        Self {
            language: "rust".into(),
            options: PlanExecVerifyOptions {
                max_fix_attempts: cfg.max_fix_attempts,
                per_step_timeout: Some(Duration::from_secs(cfg.per_step_timeout_seconds)),
                max_parallel_per_layer: cfg.max_parallel_per_layer,
                allow_single_replan: false,
                emit_checkpoints: false,
            },
            code_edit_cfg: cfg,
            critic: None,
        }
    }
}

impl CodeEditFlow {
    pub fn new(language: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            ..Self::default()
        }
    }

    pub fn with_runtime_config(language: impl Into<String>, cfg: &RuntimeConfig) -> Self {
        let code_edit = cfg.apply_model.code_edit.clone();
        Self {
            language: language.into(),
            options: PlanExecVerifyOptions {
                max_fix_attempts: code_edit.max_fix_attempts,
                per_step_timeout: Some(Duration::from_secs(code_edit.per_step_timeout_seconds)),
                max_parallel_per_layer: code_edit.max_parallel_per_layer,
                allow_single_replan: false,
                emit_checkpoints: false,
            },
            code_edit_cfg: code_edit,
            critic: None,
        }
    }

    pub fn with_critic(mut self, critic: Option<CriticContext>) -> Self {
        self.critic = critic;
        self
    }
}

#[async_trait]
impl Flow for CodeEditFlow {
    fn name(&self) -> &'static str {
        "code_edit"
    }

    async fn run(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<FlowOutcome, FlowError> {
        let inner = PlanExecVerifyFlow::new(
            "code_edit",
            CodeEditPlanner {
                cfg: self.code_edit_cfg.clone(),
            },
            CodeEditExecutor {
                language: self.language.clone(),
                cfg: self.code_edit_cfg.clone(),
            },
            CodeEditVerifier {
                critic: self.critic.clone(),
            },
        )
        .with_options(self.options.clone());

        let steps = inner.planner.plan(ctx, agent).await?;
        ctx.push(TranscriptEntry::Plan {
            steps: steps.clone(),
        });

        let layered = match build_layered_plan(ctx, &steps) {
            Some(l) => l,
            None => {
                tracing::info!(
                    target: "agent.flows.code_edit",
                    stage = "layered",
                    layers = 1u32,
                    fallback = "serial",
                    "no plan_dag in scratchpad; falling back to serial execution",
                );
                LayeredPlan::new(vec![steps])
            }
        };

        tracing::info!(
            target: "agent.flows.code_edit",
            stage = "layered",
            layer_count = layered.layers.len(),
            total_steps = layered.total_steps(),
            "layered_dispatch",
        );

        let mut outcome = inner.run_layered(ctx, agent, layered).await?;

        match run_workspace_batch_verification(ctx).await {
            Ok(true) => {
                tracing::info!(
                    target: "agent.flows.code_edit",
                    stage = "batch_verify",
                    passed = true,
                    "workspace_batch_verify_pass",
                );
            }
            Ok(false) => {
                tracing::warn!(
                    target: "agent.flows.code_edit",
                    stage = "batch_verify",
                    passed = false,
                    "workspace_batch_verify_fail",
                );
                return Err(FlowError::Verifier(
                    "batch verification failed for workspace".into(),
                ));
            }
            Err(e) => {

                tracing::warn!(
                    target: "agent.flows.code_edit",
                    stage = "batch_verify",
                    error = %e,
                    "workspace_batch_verify_infra_error",
                );
            }
        }

        outcome.transcript = ctx.transcript.clone();
        Ok(outcome)
    }
}

fn build_layered_plan(ctx: &FlowContext, steps: &[Step]) -> Option<LayeredPlan> {
    let raw = ctx.scratchpad.get("code_edit.plan_dag")?;
    let plan_steps: Vec<PlanStepJson> = match serde_json::from_str::<Vec<PlanStepJson>>(raw) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "agent.flows.code_edit",
                stage = "layered",
                error = %e,
                "code_edit.plan_dag scratchpad entry was not valid JSON; falling back to serial",
            );
            return None;
        }
    };

    let graph = match PlanDependencyGraph::build(plan_steps) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(
                target: "agent.flows.code_edit",
                stage = "layered",
                error = %e,
                "PlanDependencyGraph::build rejected scratchpad DAG; falling back to serial",
            );
            return None;
        }
    };

    let layers = match graph.topo_layers() {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(
                target: "agent.flows.code_edit",
                stage = "layered",
                error = %e,
                "topo_layers rejected DAG; falling back to serial",
            );
            return None;
        }
    };

    let mut layered: Vec<Vec<Step>> = Vec::with_capacity(layers.len());
    for layer in layers {
        let mut row: Vec<Step> = Vec::with_capacity(layer.len());
        for id in layer {
            if let Some(step) = steps.iter().find(|s| s.id == id) {
                row.push(step.clone());
            }
        }
        if !row.is_empty() {
            layered.push(row);
        }
    }
    Some(LayeredPlan::new(layered))
}

fn resolve_workspace_root(ctx: &FlowContext) -> PathBuf {
    if let Some(s) = ctx.scratchpad.get("workspace_root") {
        let p = PathBuf::from(s);
        if p.exists() {
            return std::fs::canonicalize(&p).unwrap_or(p);
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    std::fs::canonicalize(&cwd).unwrap_or(cwd)
}

async fn run_workspace_batch_verification(ctx: &FlowContext) -> anyhow::Result<bool> {
    use crate::agent::verification::VerificationPipeline;

    let root = resolve_workspace_root(ctx);
    let pipeline = VerificationPipeline::default_for_workspace(&root, None);
    let report = pipeline.run_on_workspace(&root).await?;
    Ok(report.passed)
}

struct CodeEditPlanner {
    cfg: CodeEditSection,
}

#[async_trait]
impl Planner for CodeEditPlanner {
    async fn plan(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<Vec<Step>, FlowError> {
        crate::observability::code_intel_metrics::incr_code_edit_plan_attempt();

        let workspace_root = resolve_workspace_root(ctx);
        let workspace_root_str = workspace_root.display().to_string();
        let focus_files = ctx
            .scratchpad
            .get("focus_files")
            .cloned()
            .unwrap_or_default();
        let symbol_summaries = ctx
            .scratchpad
            .get("symbol_summaries")
            .cloned()
            .unwrap_or_default();

        let prompt =
            render_planner_prompt(&ctx.goal, &workspace_root_str, &focus_files, &symbol_summaries);

        tracing::info!(
            target: "agent.flows.code_edit",
            stage = "planner",
            prompt_kind = "v2",
            "planner_dispatch",
        );

        let raw = agent.complete(&prompt).await?;
        let response = match validate_planner_response(&raw) {
            Ok(r) => r,
            Err(first_err) => {
                crate::observability::code_intel_metrics::incr_code_edit_plan_retry();
                tracing::warn!(
                    target: "agent.flows.code_edit",
                    stage = "planner",
                    attempt = 1u32,
                    error = %first_err,
                    "planner validation failed; issuing self-correct retry",
                );
                let retry_prompt = render_planner_retry_prompt(
                    truncate_for_prompt(&raw, 4_000).as_str(),
                    &first_err.to_string(),
                );
                let raw_retry = agent.complete(&retry_prompt).await?;
                match validate_planner_response(&raw_retry) {
                    Ok(r) => r,
                    Err(second_err) => {
                        crate::observability::code_intel_metrics::incr_code_edit_plan_degraded();
                        tracing::warn!(
                            target: "agent.flows.code_edit",
                            stage = "planner",
                            attempt = 2u32,
                            error = %second_err,
                            planner_degraded = true,
                            "planner_degraded",
                        );
                        return Ok(degraded_step_vec(ctx, &focus_files));
                    }
                }
            }
        };

        let mut steps_json = response.steps;

        if self.cfg.auto_expand_deps {
            let added = auto_expand_with_symbol_graph(&mut steps_json, &workspace_root);
            if added > 0 {
                crate::observability::code_intel_metrics::incr_code_edit_auto_expanded_steps(
                    added as u64,
                );
            }
        } else {
            tracing::info!(
                target: "agent.flows.code_edit",
                stage = "planner",
                auto_expand_candidates = 0u32,
                "auto_expand_deps disabled by config",
            );
        }

        let graph = match PlanDependencyGraph::build(steps_json.clone()) {
            Ok(g) => g,
            Err(e) => {
                crate::observability::code_intel_metrics::incr_code_edit_plan_degraded();
                tracing::warn!(
                    target: "agent.flows.code_edit",
                    stage = "planner",
                    error = %e,
                    planner_degraded = true,
                    "PlanDependencyGraph::build rejected expanded steps; degrading",
                );
                return Ok(degraded_step_vec(ctx, &focus_files));
            }
        };

        if let Err(e) = graph.topo_layers() {
            crate::observability::code_intel_metrics::incr_code_edit_plan_degraded();
            tracing::warn!(
                target: "agent.flows.code_edit",
                stage = "planner",
                error = %e,
                planner_degraded = true,
                "topo_layers rejected expanded steps; degrading",
            );
            return Ok(degraded_step_vec(ctx, &focus_files));
        }

        match serde_json::to_string(&steps_json) {
            Ok(s) => {
                ctx.scratchpad.insert("code_edit.plan_dag".into(), s);
            }
            Err(e) => {
                tracing::warn!(
                    target: "agent.flows.code_edit",
                    stage = "planner",
                    error = %e,
                    "could not serialise plan_dag; layered runner will fall back to serial",
                );
            }
        }

        let steps: Vec<Step> = steps_json
            .iter()
            .map(|s| step_from_plan(s, false))
            .collect();
        Ok(steps)
    }
}

fn truncate_for_prompt(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str("\n\u{2026}[truncated]\u{2026}");
    out
}

fn degraded_step_vec(ctx: &FlowContext, focus_files: &str) -> Vec<Step> {
    let default_path = focus_files
        .lines()
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown-0");
    let plan = degraded_catch_all_step(&ctx.goal, default_path);
    vec![step_from_plan(&plan, true)]
}

const CODE_EDIT_EXECUTOR_PROMPT_V2: &str = r"You are a precise code editor.

Target file: {path}
Language: {lang}
Step kind: {kind}
{snippet_block}
{outline_block}
Task: {description}

{fix_reason_block}

Respond with a SINGLE unified diff for `{path}` ONLY.  Use correct line numbers in `@@` headers (they reference the *full* file, not the snippet window).  Do NOT emit any prose, commentary, or markdown fences.";

const CODE_EDIT_FULL_FILE_PROMPT: &str = r"Edit description: {description}
Target file: {path}
Language: {lang}
Current file body:
{body}
{fix_reason_block}
Respond with ONLY the new full file body. Do not emit prose or markdown fences.";

const CODE_EDIT_REVIEW_SUFFIX: &str = r"

This is a REVIEW step.  If the upstream symbol change does NOT require modifying this file, respond with an EMPTY string.  Otherwise respond with a unified diff per the rules above.";

struct CodeEditExecutor {
    language: String,
    cfg: CodeEditSection,
}

#[async_trait]
impl Executor for CodeEditExecutor {
    async fn execute(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
        step: &Step,
    ) -> Result<ExecOutcome, FlowError> {
        let workspace_root = resolve_workspace_root(ctx);
        let rel_path = step
            .inputs
            .get("path")
            .cloned()
            .unwrap_or_else(|| step.id.clone());
        let abs_path = workspace_root.join(&rel_path);
        let kind = step
            .inputs
            .get("kind")
            .map(String::as_str)
            .unwrap_or("modify")
            .to_string();

        let fix_reason = step.inputs.get("fix_reason").cloned();

        if kind == "create" {
            return self
                .execute_create(agent, step, &workspace_root, &abs_path, fix_reason.as_deref())
                .await;
        }

        if kind == "delete" {
            return self
                .execute_delete(step, &workspace_root, &abs_path)
                .await;
        }

        if kind == "rename" {
            return self
                .execute_rename(step, &workspace_root, &abs_path)
                .await;
        }

        let body = match tokio::fs::read_to_string(&abs_path).await {
            Ok(s) => s,
            Err(e) => {
                return Err(FlowError::Executor(format!(
                    "failed to read {}: {e}",
                    abs_path.display()
                )));
            }
        };
        let line_count = body.lines().count();

        let is_review = kind == "review";

        if !is_review
            && self.cfg.full_file_rewrite_max_lines > 0
            && line_count < self.cfg.full_file_rewrite_max_lines
        {
            crate::observability::code_intel_metrics::incr_code_edit_full_file_fallback();
            tracing::warn!(
                target: "agent.flows.code_edit",
                stage = "executor",
                deprecated = "full_file_rewrite",
                path = %abs_path.display(),
                lines = line_count as u32,
                "full_file_rewrite_dispatch",
            );
            return self
                .execute_full_file_rewrite(
                    agent,
                    step,
                    &workspace_root,
                    &abs_path,
                    &body,
                    fix_reason.as_deref(),
                )
                .await;
        }

        let use_windowed = line_count >= self.cfg.window_prompt_min_lines;

        let snippet_block = if use_windowed {
            window_snippet_block(&abs_path, &body, &step.description)
        } else {
            standard_snippet_block(&body)
        };
        let outline_block = if use_windowed {
            outline_block_for(&abs_path)
        } else {
            String::new()
        };

        let mut prompt = CODE_EDIT_EXECUTOR_PROMPT_V2
            .replace("{path}", &abs_path.display().to_string())
            .replace("{lang}", &self.language)
            .replace("{kind}", &kind)
            .replace("{description}", &step.description)
            .replace("{snippet_block}", &snippet_block)
            .replace("{outline_block}", &outline_block)
            .replace("{fix_reason_block}", &render_fix_reason_block(fix_reason.as_deref()));

        if is_review {
            prompt.push_str(CODE_EDIT_REVIEW_SUFFIX);
        }

        tracing::info!(
            target: "agent.flows.code_edit",
            stage = "executor",
            kind = %kind,
            windowed = use_windowed,
            path = %abs_path.display(),
            "executor_dispatch",
        );

        let raw = agent.complete(&prompt).await?;

        if is_review && raw.trim().is_empty() {
            crate::observability::code_intel_metrics::incr_code_edit_review_noop();
            tracing::info!(
                target: "agent.flows.code_edit",
                stage = "executor",
                review_noop = true,
                path = %abs_path.display(),
                "review_noop_emitted",
            );
            let mut artifact = Artifact::new(step.id.clone(), String::new())
                .with_language(self.language.clone());
            artifact
                .metadata
                .insert("kind".into(), "review_noop".into());
            artifact.metadata.insert("review_noop".into(), "true".into());
            artifact
                .metadata
                .insert("path".into(), abs_path.display().to_string());
            return Ok(ExecOutcome::new(artifact));
        }

        let diff = match try_extract_unified_diff(&raw) {
            Some(d) => d,
            None => {
                return Err(FlowError::Executor(format!(
                    "step {} produced no extractable unified diff (raw len={})",
                    step.id,
                    raw.len()
                )));
            }
        };

        let scope_anchor: Option<crate::apply_model::edit_op::ScopeAnchor> = step
            .inputs
            .get("affected_scope")
            .and_then(|csv| csv.split(',').next().map(str::trim).map(str::to_string))
            .filter(|s| !s.is_empty())
            .map(|raw| {

                let (kind, name) = match raw.split_once(':') {
                    Some((k, n)) => (k.to_string(), n.to_string()),
                    None => ("function".to_string(), raw),
                };
                crate::apply_model::edit_op::ScopeAnchor {
                    kind,
                    name,
                    byte_range: None,
                }
            });

        let batch = EditBatch::new(EditOrigin::CodeEditFlow).with_op(EditOp::ApplyHunk {
            path: abs_path.clone(),
            diff: diff.clone(),
            fuzz: 3,
            scope_anchor,
        });
        let applier = OpsApplier::locked_for_workspace(workspace_root.clone());
        match applier.apply_batch(batch).await {
            Ok(_) => {
                crate::observability::code_intel_metrics::incr_code_edit_diff_applied();
                let new_body = tokio::fs::read_to_string(&abs_path).await.unwrap_or_default();
                let mut artifact = Artifact::new(step.id.clone(), new_body)
                    .with_language(self.language.clone());
                artifact
                    .metadata
                    .insert("kind".into(), "applied_diff".into());
                artifact
                    .metadata
                    .insert("path".into(), abs_path.display().to_string());
                artifact
                    .metadata
                    .insert("diff_len".into(), diff.len().to_string());
                Ok(ExecOutcome::new(artifact))
            }
            Err(e) => {
                let prev = serialize_previous_attempt(&diff, &e.to_string());
                Err(FlowError::Executor(format!(
                    "OpsApplier rejected diff for {}: {e}; previous_attempt={prev}",
                    abs_path.display()
                )))
            }
        }
    }
}

impl CodeEditExecutor {
    async fn execute_create(
        &self,
        agent: &dyn AgentHandle,
        step: &Step,
        workspace_root: &Path,
        abs_path: &Path,
        fix_reason: Option<&str>,
    ) -> Result<ExecOutcome, FlowError> {
        let prompt = format!(
            "Create file {path}.\nLanguage: {lang}\nTask: {desc}\n{fix}\nRespond with ONLY the new file body ??no markdown fences, no prose.",
            path = abs_path.display(),
            lang = self.language,
            desc = step.description,
            fix = render_fix_reason_block(fix_reason),
        );
        tracing::info!(
            target: "agent.flows.code_edit",
            stage = "executor",
            kind = "create",
            path = %abs_path.display(),
            "executor_dispatch",
        );
        let body = agent.complete(&prompt).await?;
        let body = strip_markdown_fence(&body).to_string();
        let batch = EditBatch::new(EditOrigin::CodeEditFlow).with_op(EditOp::CreateFile {
            path: abs_path.to_path_buf(),
            contents: body.clone(),
            overwrite: false,
            encoding: None,
            expected_pre_sha256: None,
        });
        let applier = OpsApplier::locked_for_workspace(workspace_root.to_path_buf());
        match applier.apply_batch(batch).await {
            Ok(_) => {
                crate::observability::code_intel_metrics::incr_code_edit_diff_applied();
                let mut artifact =
                    Artifact::new(step.id.clone(), body).with_language(self.language.clone());
                artifact.metadata.insert("kind".into(), "create_file".into());
                artifact
                    .metadata
                    .insert("path".into(), abs_path.display().to_string());
                Ok(ExecOutcome::new(artifact))
            }
            Err(e) => Err(FlowError::Executor(format!(
                "OpsApplier rejected create for {}: {e}",
                abs_path.display()
            ))),
        }
    }

    async fn execute_delete(
        &self,
        step: &Step,
        workspace_root: &Path,
        abs_path: &Path,
    ) -> Result<ExecOutcome, FlowError> {
        tracing::info!(
            target: "agent.flows.code_edit",
            stage = "executor",
            kind = "delete",
            path = %abs_path.display(),
            "executor_dispatch",
        );
        let batch = EditBatch::new(EditOrigin::CodeEditFlow).with_op(EditOp::DeleteFile {
            path: abs_path.to_path_buf(),
            missing_ok: false,
        });
        let applier = OpsApplier::locked_for_workspace(workspace_root.to_path_buf());
        match applier.apply_batch(batch).await {
            Ok(_) => {
                let mut artifact = Artifact::new(step.id.clone(), String::new())
                    .with_language(self.language.clone());
                artifact.metadata.insert("kind".into(), "delete_file".into());
                artifact
                    .metadata
                    .insert("path".into(), abs_path.display().to_string());
                Ok(ExecOutcome::new(artifact))
            }
            Err(e) => Err(FlowError::Executor(format!(
                "OpsApplier rejected delete for {}: {e}",
                abs_path.display()
            ))),
        }
    }

    async fn execute_rename(
        &self,
        step: &Step,
        workspace_root: &Path,
        abs_path: &Path,
    ) -> Result<ExecOutcome, FlowError> {

        let to_rel = if let Some(tp) = step.inputs.get("to_path").filter(|s| !s.is_empty()) {
            tp.clone()
        } else {
            extract_rename_target(&step.description).ok_or_else(|| {
                FlowError::Executor(format!(
                    "rename step {} has no destination path in description or to_path input",
                    step.id
                ))
            })?
        };
        let normalized_to_rel = to_rel.replace('\\', "/");
        let to_rel_path = Path::new(normalized_to_rel.as_str());
        if to_rel_path.is_absolute()
            || to_rel_path.has_root()
            || to_rel_path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(FlowError::Executor(format!(
                "rename step {} destination `{to_rel}` must be a workspace-relative path",
                step.id
            )));
        }
        let to_abs = workspace_root.join(to_rel_path);
        if to_abs == abs_path {
            return Err(FlowError::Executor(format!(
                "rename step {} resolved destination equals the source path {}",
                step.id,
                abs_path.display()
            )));
        }
        tracing::info!(
            target: "agent.flows.code_edit",
            stage = "executor",
            kind = "rename",
            from = %abs_path.display(),
            to = %to_abs.display(),
            "executor_dispatch",
        );
        let batch = EditBatch::new(EditOrigin::CodeEditFlow).with_op(EditOp::RenameFile {
            from: abs_path.to_path_buf(),
            to: to_abs.clone(),
            overwrite: false,
        });
        let applier = OpsApplier::locked_for_workspace(workspace_root.to_path_buf());
        match applier.apply_batch(batch).await {
            Ok(_) => {
                let mut artifact = Artifact::new(step.id.clone(), String::new())
                    .with_language(self.language.clone());
                artifact.metadata.insert("kind".into(), "rename_file".into());
                artifact
                    .metadata
                    .insert("path".into(), to_abs.display().to_string());
                Ok(ExecOutcome::new(artifact))
            }
            Err(e) => Err(FlowError::Executor(format!(
                "OpsApplier rejected rename {} ??{}: {e}",
                abs_path.display(),
                to_abs.display()
            ))),
        }
    }

    async fn execute_full_file_rewrite(
        &self,
        agent: &dyn AgentHandle,
        step: &Step,
        workspace_root: &Path,
        abs_path: &Path,
        body: &str,
        fix_reason: Option<&str>,
    ) -> Result<ExecOutcome, FlowError> {
        let prompt = CODE_EDIT_FULL_FILE_PROMPT
            .replace("{description}", &step.description)
            .replace("{path}", &abs_path.display().to_string())
            .replace("{lang}", &self.language)
            .replace("{body}", body)
            .replace("{fix_reason_block}", &render_fix_reason_block(fix_reason));
        let new_body = agent.complete(&prompt).await?;
        let new_body = strip_markdown_fence(&new_body).to_string();

        let byte_range = 0..body.len();
        let batch = EditBatch::new(EditOrigin::CodeEditFlow).with_op(EditOp::Replace {
            path: abs_path.to_path_buf(),
            byte_range,
            old_text: body.to_string(),
            new_text: new_body.clone(),
            anchor: None,
        });
        let applier = OpsApplier::locked_for_workspace(workspace_root.to_path_buf());
        match applier.apply_batch(batch).await {
            Ok(_) => {
                let mut artifact = Artifact::new(step.id.clone(), new_body)
                    .with_language(self.language.clone());
                artifact
                    .metadata
                    .insert("kind".into(), "full_file_rewrite".into());
                artifact
                    .metadata
                    .insert("path".into(), abs_path.display().to_string());
                artifact
                    .metadata
                    .insert("deprecated".into(), "full_file_rewrite".into());
                Ok(ExecOutcome::new(artifact))
            }
            Err(e) => Err(FlowError::Executor(format!(
                "OpsApplier rejected full-file rewrite for {}: {e}",
                abs_path.display()
            ))),
        }
    }
}

fn standard_snippet_block(body: &str) -> String {
    format!(
        "Current file body:\n```\n{body}\n```",
        body = body
    )
}

fn window_snippet_block(path: &Path, body: &str, description: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    if lines.is_empty() {
        return "Current file body: (empty)".into();
    }

    let outline = extract_outline(path, None).unwrap_or_default();
    let needle = first_identifier_from(description);
    let center = needle
        .as_deref()
        .and_then(|n| outline.iter().find(|e| e.name == n))
        .map(|e| e.line as usize)
        .unwrap_or(1);

    let total_lines = lines.len();
    let half = 25usize;
    let center = center.min(total_lines).max(1);
    let start = center.saturating_sub(half).max(1).min(total_lines);
    let end = center.saturating_add(half).min(total_lines).max(start);
    let snippet = lines[(start - 1)..end].join("\n");
    format!(
        "Current file (lines {start}-{end} of {total}):\n```\n{snippet}\n```",
        total = lines.len()
    )
}

fn outline_block_for(path: &Path) -> String {
    let outline = extract_outline(path, None).unwrap_or_default();
    if outline.is_empty() {
        return String::new();
    }
    let mut s = String::from("Outline (line numbers reference the full file):\n");
    for entry in outline.iter().take(64) {
        s.push_str(&format!("- {} {} @ line {}\n", entry.kind, entry.name, entry.line));
    }
    s
}

fn render_fix_reason_block(reason: Option<&str>) -> String {
    match reason {
        Some(r) if !r.trim().is_empty() => format!(
            "Previous attempt failed.  Diagnostics (JSON):\n{r}\nApply a minimal correction; do not rewrite unrelated regions.",
        ),
        _ => String::new(),
    }
}

fn first_identifier_from(text: &str) -> Option<String> {
    for tok in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if tok.len() < 3 {
            continue;
        }
        let first = tok.chars().next()?;
        if first.is_ascii_alphabetic() || first == '_' {
            return Some(tok.to_string());
        }
    }
    None
}

fn clean_path_token(raw: &str) -> String {
    raw.trim_start_matches(['`', '"', '\''])
        .trim_end_matches(['`', '"', '\'', ',', '.', ';', ':', ')', ']'])
        .to_string()
}

fn path_tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split_whitespace()
        .filter(|t| {
            !t.is_empty()
                && (t.contains('/') || t.contains('\\'))
                && !t.starts_with('-')
                && !t.starts_with('@')
        })
        .map(clean_path_token)
        .filter(|t| !t.is_empty())
}

fn extract_rename_target(text: &str) -> Option<String> {
    for sep in [" to ", "->", "→", " as "] {
        if let Some(pos) = text.find(sep) {
            if let Some(tok) = path_tokens(&text[pos + sep.len()..]).next() {
                return Some(tok);
            }
        }
    }
    path_tokens(text).last()
}

fn strip_markdown_fence(s: &str) -> &str {
    let s = s.trim();
    let prefixes = ["```rust", "```python", "```typescript", "```javascript", "```json", "```"];
    for p in prefixes {
        if let Some(rest) = s.strip_prefix(p) {
            if let Some(inner) = rest.trim_start().strip_suffix("```") {
                return inner.trim();
            }
        }
    }
    s
}

pub(crate) fn try_extract_unified_diff(raw: &str) -> Option<String> {
    let s = raw.trim();

    let unfenced = if let Some(rest) = s.strip_prefix("```diff") {
        rest.trim_start()
            .strip_suffix("```")
            .map(|x| x.trim())
            .unwrap_or(rest)
    } else if let Some(rest) = s.strip_prefix("```") {
        rest.trim_start()
            .strip_suffix("```")
            .map(|x| x.trim())
            .unwrap_or(rest)
    } else {
        s
    };

    if unfenced.contains("@@") || unfenced.contains("--- ") || unfenced.contains("+++ ") {
        Some(unfenced.to_string())
    } else {
        None
    }
}

fn serialize_previous_attempt(diff: &str, error: &str) -> String {
    #[derive(Serialize)]
    struct Prev<'a> {
        diff: &'a str,
        error: &'a str,
    }
    serde_json::to_string(&Prev { diff, error }).unwrap_or_else(|_| "{}".into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {

    SyntaxError,

    CompileOrTestError,

    LspDiagnostic,

    DiagnosticFailure,

    CriticRejected,

    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuredFixReason {
    pub failure: FailureKind,
    pub diagnostics: Vec<FixDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempted_diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_error: Option<String>,
    pub failed_stages: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixDiagnostic {
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub stage: String,
}

struct CodeEditVerifier {
    critic: Option<CriticContext>,
}

#[async_trait]
impl Verifier for CodeEditVerifier {
    async fn verify(
        &self,
        ctx: &mut FlowContext,
        artifact: &Artifact,
    ) -> Result<VerificationVerdict, FlowError> {

        if artifact.metadata.get("review_noop").map(String::as_str) == Some("true")
            || artifact.metadata.get("kind").map(String::as_str) == Some("review_noop")
        {
            tracing::info!(
                target: "agent.flows.code_edit",
                stage = "verifier",
                review_noop = true,
                "review_noop_short_circuit",
            );
            return Ok(VerificationVerdict::Pass);
        }

        use crate::agent::verification::traits::{
            Artifact as VArtifact, ArtifactKind, Language as VLanguage,
        };
        use crate::agent::verification::VerificationPipeline;

        let lang = match artifact.language.as_deref() {
            Some("rust") => VLanguage::Rust,
            Some("python") | Some("py") => VLanguage::Python,
            Some("typescript") | Some("ts") => VLanguage::TypeScript,
            Some("javascript") | Some("js") => VLanguage::JavaScript,
            Some("json") => VLanguage::Json,
            Some("toml") => VLanguage::Toml,
            Some("md") | Some("markdown") => VLanguage::Markdown,
            Some("go") => VLanguage::Go,
            Some("java") => VLanguage::Java,
            Some("c") => VLanguage::C,
            Some("cpp") | Some("c++") | Some("cxx") => VLanguage::Cpp,
            _ => VLanguage::Unknown,
        };

        let path = artifact
            .metadata
            .get("path")
            .cloned()
            .unwrap_or_else(|| artifact.step_id.clone());
        let v_artifact = VArtifact {
            kind: ArtifactKind::Patch,
            path: path.clone().into(),
            contents: artifact.content.clone(),
            language: lang,
        };

        let root = resolve_workspace_root(ctx);
        let pipeline = VerificationPipeline::default_for_workspace(&root, None);

        tracing::info!(
            target: "agent.flows.code_edit",
            stage = "verifier",
            stages = "stage=syntactic,test_runner,lsp_diag",
            policy = "collect_all",
            stage_count = pipeline.stage_count(),
            "verification_pipeline_dispatch",
        );

        let report = pipeline
            .run(&v_artifact)
            .await
            .map_err(|e| FlowError::Verifier(e.to_string()))?;
        if report.passed {
            if let Some(critic) = self.critic.as_ref().filter(|c| c.is_code_edit_review_enabled()) {
                if let Some(verdict) = crate::agent::self_assess::critic::IndependentCritic::review_code_edit(
                    critic,
                    &ctx.goal,
                    &path,
                    &artifact.content,
                )
                .await
                {
                    if verdict.should_retry {
                        let diagnostics: Vec<FixDiagnostic> = verdict
                            .findings
                            .iter()
                            .map(|f| FixDiagnostic {
                                severity: f.severity.clone(),
                                message: f.message.clone(),
                                path: Some(path.clone()),
                                line: None,
                                column: None,
                                stage: "independent_critic".to_string(),
                            })
                            .collect();
                        tracing::info!(
                            target: "agent.flows.code_edit",
                            stage = "critic",
                            score = verdict.score,
                            findings = diagnostics.len(),
                            "independent_critic_rejected",
                        );
                        let reason_payload = StructuredFixReason {
                            failure: FailureKind::CriticRejected,
                            diagnostics,
                            attempted_diff: None,
                            previous_error: if verdict.rationale.is_empty() {
                                None
                            } else {
                                Some(verdict.rationale)
                            },
                            failed_stages: vec!["independent_critic".to_string()],
                        };
                        let reason_json = serde_json::to_string(&reason_payload)
                            .unwrap_or_else(|_| "{\"failure\":\"critic_rejected\"}".into());
                        return Ok(VerificationVerdict::Fail {
                            reason: reason_json,
                            retryable: true,
                        });
                    }
                }
            }
            return Ok(VerificationVerdict::Pass);
        }

        let attempted_diff = artifact.metadata.get("kind").and_then(|k| {
            if k == "applied_diff" {

                let renderer = crate::apply_model::UnifiedHunkRenderer::new()
                    .with_scope_annotation(true);
                Some(renderer.render(std::path::Path::new(&path), &artifact.content))
            } else {
                None
            }
        });
        let mut diagnostics: Vec<FixDiagnostic> = Vec::new();
        for stage_report in &report.reports {
            for issue in &stage_report.issues {
                diagnostics.push(FixDiagnostic {
                    severity: format!("{:?}", issue.severity).to_lowercase(),
                    message: issue.message.clone(),
                    path: Some(path.clone()),
                    line: Some(issue.line),
                    column: Some(issue.column),
                    stage: stage_report.verifier.to_string(),
                });
            }
        }
        let failed_stages_owned: Vec<String> =
            report.failed_stages.iter().map(|s| (*s).to_string()).collect();
        let failure = classify_failure(&diagnostics, &failed_stages_owned);
        let reason_payload = StructuredFixReason {
            failure,
            diagnostics,
            attempted_diff,
            previous_error: None,
            failed_stages: failed_stages_owned,
        };
        let reason_json = serde_json::to_string(&reason_payload)
            .unwrap_or_else(|_| "{\"failure\":\"unknown\"}".into());
        Ok(VerificationVerdict::Fail {
            reason: reason_json,
            retryable: true,
        })
    }
}

fn classify_failure(diagnostics: &[FixDiagnostic], failed_stages: &[String]) -> FailureKind {
    if failed_stages.iter().any(|s| s == "syntactic") {
        return FailureKind::SyntaxError;
    }
    if failed_stages.iter().any(|s| s == "test_runner") {
        return FailureKind::CompileOrTestError;
    }
    if failed_stages.iter().any(|s| s == "lsp_diag") {
        return FailureKind::LspDiagnostic;
    }
    if !diagnostics.is_empty() {
        return FailureKind::DiagnosticFailure;
    }
    FailureKind::Unknown
}

pub struct ResearchFlow {
    pub min_sources: usize,
    pub options: PlanExecVerifyOptions,
}

impl Default for ResearchFlow {
    fn default() -> Self {
        Self {
            min_sources: 1,
            options: PlanExecVerifyOptions::default(),
        }
    }
}

#[async_trait]
impl Flow for ResearchFlow {
    fn name(&self) -> &'static str {
        "research"
    }

    async fn run(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<FlowOutcome, FlowError> {
        let inner = PlanExecVerifyFlow::new(
            "research",
            ResearchPlanner,
            ResearchExecutor,
            ResearchVerifier {
                min_sources: self.min_sources,
            },
        )
        .with_options(self.options.clone());
        inner.run(ctx, agent).await
    }
}

struct ResearchPlanner;

#[async_trait]
impl Planner for ResearchPlanner {
    async fn plan(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<Vec<Step>, FlowError> {
        let prompt = format!(
            "Research goal:\n{}\n\nReturn one research question per line.",
            ctx.goal
        );
        let raw = agent.complete(&prompt).await?;
        let steps: Vec<Step> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, line)| Step::new(format!("q-{i}"), line.trim().to_string()))
            .collect();
        if steps.is_empty() {
            Ok(vec![Step::new("q-0", ctx.goal.clone())])
        } else {
            Ok(steps)
        }
    }
}

struct ResearchExecutor;

#[async_trait]
impl Executor for ResearchExecutor {
    async fn execute(
        &self,
        _ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
        step: &Step,
    ) -> Result<ExecOutcome, FlowError> {
        let fix_hint = step
            .inputs
            .get("fix_reason")
            .map(|r| format!("\nRevise because: {r}"))
            .unwrap_or_default();
        let prompt = format!(
            "Question: {}\nAnswer concisely and include `[source: ...]` citations.{fix_hint}",
            step.description
        );
        let body = agent.complete(&prompt).await?;
        Ok(ExecOutcome::new(Artifact::new(step.id.clone(), body)))
    }
}

struct ResearchVerifier {
    min_sources: usize,
}

#[async_trait]
impl Verifier for ResearchVerifier {
    async fn verify(
        &self,
        _ctx: &mut FlowContext,
        artifact: &Artifact,
    ) -> Result<VerificationVerdict, FlowError> {
        let content = artifact.content.trim();
        if content.is_empty() {
            return Ok(VerificationVerdict::Fail {
                reason: "empty answer".into(),
                retryable: true,
            });
        }
        let source_count = content.matches("[source:").count();
        if source_count < self.min_sources {
            return Ok(VerificationVerdict::Fail {
                reason: format!(
                    "answer has {source_count} citation(s), need >= {}",
                    self.min_sources
                ),
                retryable: true,
            });
        }
        Ok(VerificationVerdict::Pass)
    }
}
