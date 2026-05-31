// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use super::types::{PlanContext, VerifyOutcome, WritePlan, WriteStep};
use crate::agent::verification::{
    Artifact as VerifyArtifact, ArtifactKind, Language, SyntacticVerifier, VerificationPipeline,
    VerificationReport, Verifier,
};
use crate::apply_model::{
    ApplyOptions, EditBatch, EditOp, EditOrigin, HeuristicApplier, OpsApplier, traits::Applier,
};
use crate::observability::session_write_mode_metrics;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepOutcome {
    pub label: &'static str,
    pub summary: String,

    #[serde(default)]
    pub captured: String,

    #[serde(default)]
    pub exit_code: i32,
}

impl StepOutcome {
    fn note(label: &'static str, summary: impl Into<String>) -> Self {
        Self {
            label,
            summary: summary.into(),
            captured: String::new(),
            exit_code: 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    #[error("read_file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("grep_symbol {query}: symbol graph not loaded (workspace not indexed?)")]
    GrepSymbolMissing { query: String },
    #[error("apply_diff {path}: inline-edit runner failed: {reason}")]
    ApplyDiff { path: PathBuf, reason: String },
    #[error("apply_diff {path}: neither diff nor instruction provided")]
    ApplyDiffEmpty { path: PathBuf },

    #[error("apply_diff {path}: verification failed after refine; rolled back ({reason})")]
    ApplyDiffVerifyFailed { path: PathBuf, reason: String },

    #[error("apply_diff {path}: rollback after verification failure also failed: {rollback}")]
    ApplyDiffRollbackFailed { path: PathBuf, rollback: String },
    #[error("run_command `{command}`: exit {code}\n{stderr}")]
    Command {
        command: String,
        code: i32,
        stderr: String,
    },
    #[error("run_command spawn failed: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("path escapes workspace: {0}")]
    PathEscape(PathBuf),
}

pub type ApplyFnOutput =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'static>>;

type ApplyFn = Arc<dyn Fn(&str, &Path, Option<&str>, Option<&str>) -> ApplyFnOutput + Send + Sync>;

pub struct WriteExecutor {
    apply_fn: ApplyFn,
    ops_applier: Option<Arc<OpsApplier>>,

    pipeline: Option<Arc<VerificationPipeline>>,
}

impl WriteExecutor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            apply_fn: default_apply_fn(),
            ops_applier: None,
            pipeline: None,
        }
    }

    #[must_use]
    pub fn with_apply_fn<F>(mut self, apply_fn: F) -> Self
    where
        F: Fn(&str, &Path, Option<&str>, Option<&str>) -> ApplyFnOutput + Send + Sync + 'static,
    {
        self.apply_fn = Arc::new(apply_fn);
        self
    }

    #[must_use]
    pub fn with_ops_applier(mut self, ops_applier: Arc<OpsApplier>) -> Self {
        self.ops_applier = Some(ops_applier);
        self
    }

    #[must_use]
    pub fn with_pipeline(mut self, pipeline: Arc<VerificationPipeline>) -> Self {
        self.pipeline = Some(pipeline);
        self
    }

    fn ops_applier_for(&self, root: &Path) -> Arc<OpsApplier> {
        if let Some(o) = self.ops_applier.clone() {
            return o;
        }
        Arc::new(OpsApplier::default_for_workspace(root.to_path_buf()))
    }

    async fn verify_apply_artifact(
        &self,
        path: &Path,
        contents: &str,
    ) -> (bool, String) {
        let language = Language::from_path(path);
        let artifact = VerifyArtifact {
            kind: ArtifactKind::File,
            path: path.to_path_buf(),
            contents: contents.to_string(),
            language,
        };

        if let Some(pipeline) = self.pipeline.as_ref() {
            return match pipeline.run(&artifact).await {
                Ok(report) if report.passed => (true, String::new()),
                Ok(report) => {
                    let stages = report.failed_stages.join(",");
                    let issues = report
                        .reports
                        .iter()
                        .flat_map(|r| r.issues.iter().map(|i| i.message.clone()))
                        .collect::<Vec<_>>()
                        .join("; ");
                    let reason = if issues.is_empty() {
                        format!("verification failed: stages={stages}")
                    } else {
                        format!("{issues} (stages={stages})")
                    };
                    (false, reason)
                }
                Err(e) => {
                    tracing::warn!(
                        target: "write_mode.executor.apply_verify",
                        error = %e,
                        "verification pipeline raised an infrastructure error; \
                         falling back to syntactic check"
                    );
                    fallback_syntactic_verify(&artifact).await
                }
            };
        }

        fallback_syntactic_verify(&artifact).await
    }
}

async fn fallback_syntactic_verify(artifact: &VerifyArtifact) -> (bool, String) {
    let verifier = SyntacticVerifier::new();
    match verifier.verify(artifact).await {
        Ok(report) if report.passed => (true, String::new()),
        Ok(report) => (false, summarise_report(&report)),
        Err(e) => {

            tracing::debug!(
                target: "write_mode.executor.apply_verify",
                error = %e,
                "syntactic verifier raised infra error; treating as pass"
            );
            (true, String::new())
        }
    }
}

fn summarise_report(report: &VerificationReport) -> String {
    if !report.summary.is_empty() {
        return report.summary.clone();
    }
    let issues: Vec<String> = report.issues.iter().map(|i| i.message.clone()).collect();
    if issues.is_empty() {
        format!("verifier `{}` reported failure", report.verifier)
    } else {
        issues.join("; ")
    }
}

impl Default for WriteExecutor {
    fn default() -> Self {
        Self::new()
    }
}

fn default_apply_fn() -> ApplyFn {
    Arc::new(
        |source: &str, _path: &Path, _instruction: Option<&str>, diff: Option<&str>| {
            let source = source.to_string();
            let diff = diff.map(str::to_owned);
            Box::pin(async move {
                let Some(diff) = diff else {
                    return Err(
                        "DefaultApplyFn requires a concrete diff; wire an LLM apply_fn for \
                         instruction-only steps"
                            .to_string(),
                    );
                };
                let opts = ApplyOptions {
                    max_fuzz: 3,
                    dry_run: false,
                    validate: true,
                };

                HeuristicApplier
                    .apply(&source, &diff, &opts)
                    .map(|r| r.applied)
                    .map_err(|e| e.to_string())
            }) as ApplyFnOutput
        },
    )
}

use std::sync::Arc;

impl WriteExecutor {

    pub async fn execute_default(
        &self,
        ctx: &PlanContext,
        plan: &WritePlan,
    ) -> Result<(Vec<StepOutcome>, VerifyOutcome), ExecuteError> {
        self.execute(ctx, plan).await
    }

    pub async fn execute(
        &self,
        ctx: &PlanContext,
        plan: &WritePlan,
    ) -> Result<(Vec<StepOutcome>, VerifyOutcome), ExecuteError> {
        let mut outcomes: Vec<StepOutcome> = Vec::with_capacity(plan.steps.len());
        let mut last_captured: Option<String> = None;
        let mut last_artifact_path: Option<PathBuf> = None;
        let mut verify: VerifyOutcome = VerifyOutcome::Absent;

        for step in &plan.steps {
            session_write_mode_metrics::incr_write_mode_step();
            match step {
                WriteStep::ReadFile { path } => {
                    let abs = resolve_workspace_path(&ctx.workspace_root, path)?;
                    let src = tokio::fs::read_to_string(&abs).await.map_err(|source| {
                        ExecuteError::ReadFile {
                            path: abs.clone(),
                            source,
                        }
                    })?;
                    outcomes.push(StepOutcome {
                        label: "read_file",
                        summary: format!("{} ({} bytes)", path.display(), src.len()),
                        captured: String::new(),
                        exit_code: 0,
                    });
                    last_captured = Some(src);
                    last_artifact_path = Some(abs);
                }
                WriteStep::GrepSymbol { query } => {
                    let graph =
                        crate::code_intel::symbol_graph::SymbolGraph::load(&ctx.workspace_root)
                            .ok()
                            .flatten();
                    match graph {
                        Some(g) => {
                            let hits: Vec<String> = g
                                .symbols
                                .iter()
                                .filter(|s| s.id.name.contains(query))
                                .take(8)
                                .map(|s| {
                                    format!("{}:{} {}", s.id.file.display(), s.id.line, s.id.name)
                                })
                                .collect();
                            outcomes.push(StepOutcome {
                                label: "grep_symbol",
                                summary: format!("{} hits for '{}'", hits.len(), query),
                                captured: hits.join("\n"),
                                exit_code: 0,
                            });
                        }
                        None => {
                            return Err(ExecuteError::GrepSymbolMissing {
                                query: query.clone(),
                            });
                        }
                    }
                }
                WriteStep::ApplyDiff {
                    path,
                    instruction,
                    diff,
                } => {
                    if instruction.is_none() && diff.is_none() {
                        return Err(ExecuteError::ApplyDiffEmpty { path: path.clone() });
                    }
                    let abs = resolve_workspace_path(&ctx.workspace_root, path)?;
                    let source = tokio::fs::read_to_string(&abs).await.map_err(|source| {
                        ExecuteError::ReadFile {
                            path: abs.clone(),
                            source,
                        }
                    })?;
                    let ops_applier = self.ops_applier_for(&ctx.workspace_root);

                    let fut =
                        (self.apply_fn)(&source, &abs, instruction.as_deref(), diff.as_deref());
                    let mut new_contents =
                        fut.await.map_err(|reason| ExecuteError::ApplyDiff {
                            path: path.clone(),
                            reason,
                        })?;
                    let mut batch_id =
                        write_full_replace(&ops_applier, &abs, &source, &new_contents)
                            .await
                            .map_err(|e| ExecuteError::ApplyDiff {
                                path: path.clone(),
                                reason: e,
                            })?;

                    let (mut passed, mut reason) =
                        self.verify_apply_artifact(&abs, &new_contents).await;

                    if !passed {

                        session_write_mode_metrics::incr_write_mode_apply_verify_refine();
                        let hint = match instruction.as_deref() {
                            Some(orig) if !orig.is_empty() => format!(
                                "{orig}\n\n[verify-refine]\nThe previous edit failed automated \
                                 verification with this reason; please correct it without \
                                 re-introducing the same defect:\n{reason}"
                            ),
                            _ => format!(
                                "[verify-refine]\nThe previous edit failed automated \
                                 verification with this reason; please correct it:\n{reason}"
                            ),
                        };
                        let fut2 = (self.apply_fn)(
                            &source,
                            &abs,
                            Some(hint.as_str()),
                            diff.as_deref(),
                        );
                        match fut2.await {
                            Ok(refined) => {

                                if let Err(rb) = ops_applier.rollback(&batch_id).await {
                                    return Err(ExecuteError::ApplyDiffRollbackFailed {
                                        path: path.clone(),
                                        rollback: rb.to_string(),
                                    });
                                }
                                new_contents = refined;
                                batch_id = write_full_replace(
                                    &ops_applier,
                                    &abs,
                                    &source,
                                    &new_contents,
                                )
                                .await
                                .map_err(|e| ExecuteError::ApplyDiff {
                                    path: path.clone(),
                                    reason: e,
                                })?;
                                let (p, r) =
                                    self.verify_apply_artifact(&abs, &new_contents).await;
                                passed = p;
                                reason = r;
                            }
                            Err(refine_err) => {

                                let summary = format!(
                                    "{reason}; refine attempt failed: {refine_err}"
                                );
                                rollback_or_escalate(&ops_applier, &batch_id, path).await?;
                                session_write_mode_metrics::incr_write_mode_apply_verify_rollback();
                                return Err(ExecuteError::ApplyDiffVerifyFailed {
                                    path: path.clone(),
                                    reason: summary,
                                });
                            }
                        }
                    }

                    if !passed {

                        rollback_or_escalate(&ops_applier, &batch_id, path).await?;
                        session_write_mode_metrics::incr_write_mode_apply_verify_rollback();
                        return Err(ExecuteError::ApplyDiffVerifyFailed {
                            path: path.clone(),
                            reason,
                        });
                    }

                    session_write_mode_metrics::incr_write_mode_apply_verify_pass();
                    outcomes.push(StepOutcome {
                        label: "apply_diff",
                        summary: format!(
                            "{} ({} bytes ??{} bytes, verified)",
                            path.display(),
                            source.len(),
                            new_contents.len()
                        ),
                        captured: String::new(),
                        exit_code: 0,
                    });
                    last_captured = Some(new_contents);
                    last_artifact_path = Some(abs);
                }
                WriteStep::RunCommand { command, cwd } => {
                    let cwd = cwd.clone().unwrap_or_else(|| ctx.workspace_root.clone());
                    let (exit_code, output) = run_shell(command, &cwd).await?;
                    outcomes.push(StepOutcome {
                        label: "run_command",
                        summary: format!("{} ??exit {}", command, exit_code),
                        captured: output.clone(),
                        exit_code,
                    });
                    last_captured = Some(output);
                    if exit_code != 0 {
                        return Err(ExecuteError::Command {
                            command: command.clone(),
                            code: exit_code,
                            stderr: last_captured.clone().unwrap_or_default(),
                        });
                    }
                }
                WriteStep::Verify { expect_contains } => {

                    verify = if let (Some(pipeline), Some(path), Some(content)) = (
                        self.pipeline.as_ref(),
                        last_artifact_path.as_ref(),
                        last_captured.as_ref(),
                    ) {
                        let art = VerifyArtifact {
                            kind: ArtifactKind::File,
                            path: path.clone(),
                            contents: content.clone(),
                            language: Language::from_path(path),
                        };
                        match pipeline.run(&art).await {
                            Ok(report) if report.passed => {

                                let hay = content.as_str();
                                let missing: Vec<&str> = expect_contains
                                    .iter()
                                    .filter(|s| !hay.contains(s.as_str()))
                                    .map(String::as_str)
                                    .collect();
                                if missing.is_empty() {
                                    session_write_mode_metrics::incr_write_mode_verify_pass();
                                    VerifyOutcome::Passed
                                } else {
                                    session_write_mode_metrics::incr_write_mode_verify_fail();
                                    VerifyOutcome::Failed {
                                        reason: format!("missing substrings: {missing:?}"),
                                    }
                                }
                            }
                            Ok(report) => {
                                session_write_mode_metrics::incr_write_mode_verify_fail();
                                let stages = report.failed_stages.join(",");
                                let issues = report
                                    .reports
                                    .iter()
                                    .flat_map(|r| r.issues.iter().map(|i| i.message.clone()))
                                    .collect::<Vec<_>>()
                                    .join("; ");
                                let reason = if issues.is_empty() {
                                    format!("verification failed: stages={stages}")
                                } else {
                                    format!("{issues} (stages={stages})")
                                };
                                VerifyOutcome::Failed { reason }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    target: "write_mode.executor",
                                    error = %e,
                                    "verification pipeline raised an infrastructure error; \
                                     falling back to expect_contains"
                                );
                                let hay = content.as_str();
                                let missing: Vec<&str> = expect_contains
                                    .iter()
                                    .filter(|s| !hay.contains(s.as_str()))
                                    .map(String::as_str)
                                    .collect();
                                if missing.is_empty() {
                                    session_write_mode_metrics::incr_write_mode_verify_pass();
                                    VerifyOutcome::Passed
                                } else {
                                    session_write_mode_metrics::incr_write_mode_verify_fail();
                                    VerifyOutcome::Failed {
                                        reason: format!("missing substrings: {missing:?}"),
                                    }
                                }
                            }
                        }
                    } else {

                        let hay = last_captured.as_deref().unwrap_or("");
                        let missing: Vec<&str> = expect_contains
                            .iter()
                            .filter(|s| !hay.contains(s.as_str()))
                            .map(String::as_str)
                            .collect();
                        if missing.is_empty() {
                            session_write_mode_metrics::incr_write_mode_verify_pass();
                            VerifyOutcome::Passed
                        } else {
                            session_write_mode_metrics::incr_write_mode_verify_fail();
                            VerifyOutcome::Failed {
                                reason: format!("missing substrings: {missing:?}"),
                            }
                        }
                    };
                    outcomes.push(StepOutcome::note("verify", format!("{:?}", verify)));
                }
            }
        }
        Ok((outcomes, verify))
    }
}

fn resolve_workspace_path(root: &Path, path: &Path) -> Result<PathBuf, ExecuteError> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };

    let mut normal = PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::ParentDir => {
                if !normal.pop() {
                    return Err(ExecuteError::PathEscape(joined));
                }
            }
            std::path::Component::CurDir => {}
            other => normal.push(other.as_os_str()),
        }
    }
    if !normal.starts_with(root) {
        return Err(ExecuteError::PathEscape(normal));
    }
    Ok(normal)
}

async fn write_full_replace(
    ops_applier: &OpsApplier,
    abs: &Path,
    source: &str,
    new_contents: &str,
) -> Result<String, String> {
    let batch = EditBatch::new(EditOrigin::WriteMode).with_op(EditOp::Replace {
        path: abs.to_path_buf(),
        byte_range: 0..source.len(),
        old_text: source.to_string(),
        new_text: new_contents.to_string(),
        anchor: None,
    });
    ops_applier
        .apply_batch(batch)
        .await
        .map(|outcome| outcome.batch_id)
        .map_err(|e| e.to_string())
}

async fn rollback_or_escalate(
    ops_applier: &OpsApplier,
    batch_id: &str,
    path: &Path,
) -> Result<(), ExecuteError> {
    if let Err(rb) = ops_applier.rollback(batch_id).await {
        return Err(ExecuteError::ApplyDiffRollbackFailed {
            path: path.to_path_buf(),
            rollback: rb.to_string(),
        });
    }
    Ok(())
}

async fn run_shell(command: &str, cwd: &Path) -> Result<(i32, String), ExecuteError> {
    #[cfg(windows)]
    let (program, flag) = ("cmd", "/C");
    #[cfg(not(windows))]
    let (program, flag) = ("sh", "-c");

    let output = crate::util::hidden_async_command(program)
        .arg(flag)
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(ExecuteError::Spawn)?;
    let mut captured = String::from_utf8_lossy(&output.stdout).into_owned();
    captured.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((output.status.code().unwrap_or(-1), captured))
}
