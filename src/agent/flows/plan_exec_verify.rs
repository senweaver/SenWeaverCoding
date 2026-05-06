// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! The `PlanExecVerify` combinator ??the canonical flow shape that
//! drives a plan ??execute ??verify ??fix loop.
//!
//! added two complementary entry points on top of the
//! original `run_once` shape:
//!
//! 1. [`PlanExecVerifyOptions::per_step_timeout`] ??bounds each
//!    `(execute, verify, fix)` cycle so a stuck LLM cannot wedge the
//!    flow.
//! 2. [`PlanExecVerifyFlow::run_layered`] ??schedules independent
//!    plan steps concurrently using
//!    [`futures_util::stream::FuturesUnordered`] gated by a
//!    [`tokio::sync::Semaphore`].  Layers are derived from
//!    [`crate::agent::flows::code_edit_plan::PlanDependencyGraph::topo_layers`]
//!    and emitted by `CodeEditFlow` via
//!    `ctx.scratchpad["code_edit.plan_dag"]`.  We deliberately use
//!    `FuturesUnordered` (not `tokio::JoinSet`) so the runner can
//!    borrow `&self` without forcing the executor / verifier to be
//!    `'static` ??the layered runner shares the flow's owned
//!    executor / verifier for free.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::timeout;

use super::checkpoint::Checkpoint;
use super::registry::global_checkpoint_store;
use super::traits::{
    AgentHandle, Artifact, Executor, Flow, FlowContext, FlowError, FlowOutcome, Planner, Step,
    TranscriptEntry, VerificationVerdict, Verifier,
};

#[derive(Debug, Clone)]
pub struct PlanExecVerifyOptions {

    pub max_fix_attempts: u32,

    pub allow_single_replan: bool,

    pub emit_checkpoints: bool,

    pub per_step_timeout: Option<Duration>,

    pub max_parallel_per_layer: usize,
}

impl Default for PlanExecVerifyOptions {
    fn default() -> Self {
        Self {
            max_fix_attempts: 3,
            allow_single_replan: false,
            emit_checkpoints: false,
            per_step_timeout: None,
            max_parallel_per_layer: 1,
        }
    }
}

pub struct PlanExecVerifyFlow<P, E, V> {
    pub name: &'static str,
    pub planner: P,
    pub executor: E,
    pub verifier: V,
    pub options: PlanExecVerifyOptions,
}

impl<P, E, V> PlanExecVerifyFlow<P, E, V>
where
    P: Planner,
    E: Executor,
    V: Verifier,
{
    pub fn new(name: &'static str, planner: P, executor: E, verifier: V) -> Self {
        Self {
            name,
            planner,
            executor,
            verifier,
            options: PlanExecVerifyOptions::default(),
        }
    }

    pub fn with_options(mut self, options: PlanExecVerifyOptions) -> Self {
        self.options = options;
        self
    }

    async fn run_once(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<(Vec<Artifact>, u32), FlowError> {
        let steps = self.planner.plan(ctx, agent).await?;
        ctx.push(TranscriptEntry::Plan {
            steps: steps.clone(),
        });

        let mut artifacts: Vec<Artifact> = Vec::with_capacity(steps.len());
        let mut total_iterations: u32 = 0;
        for step in steps {
            let (artifact, attempts) = self.execute_step_with_fix(ctx, agent, &step).await?;
            total_iterations = total_iterations.saturating_add(attempts);
            artifacts.push(artifact.clone());
            if self.options.emit_checkpoints {
                let cp = Checkpoint::new(
                    format!("{}::{}", self.name, step.id),
                    step.description.clone(),
                    artifacts.clone(),
                    ctx.transcript.clone(),
                );
                global_checkpoint_store().push(cp);
            }
        }
        Ok((artifacts, total_iterations))
    }

    async fn execute_step_with_fix(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
        step: &Step,
    ) -> Result<(Artifact, u32), FlowError> {
        if let Some(t) = self.options.per_step_timeout {

            match timeout(t, self.execute_step_with_fix_inner(ctx, agent, step)).await {
                Ok(res) => res,
                Err(_) => Err(FlowError::Other(format!(
                    "step {} timed out after {}s",
                    step.id,
                    t.as_secs()
                ))),
            }
        } else {
            self.execute_step_with_fix_inner(ctx, agent, step).await
        }
    }

    async fn execute_step_with_fix_inner(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
        step: &Step,
    ) -> Result<(Artifact, u32), FlowError> {

        let mut outcome = self.executor.execute(ctx, agent, step).await?;
        ctx.push(TranscriptEntry::Exec {
            step_id: step.id.clone(),
            artifact: outcome.artifact.clone(),
        });

        let mut attempts: u32 = 1;
        loop {
            let verdict = self.verifier.verify(ctx, &outcome.artifact).await?;
            ctx.push(TranscriptEntry::Verify {
                step_id: step.id.clone(),
                verdict: verdict.clone(),
            });
            match verdict {
                VerificationVerdict::Pass => {
                    return Ok((outcome.artifact, attempts));
                }
                VerificationVerdict::Fail {
                    reason,
                    retryable: false,
                } => {
                    return Err(FlowError::Verifier(format!(
                        "step {} failed non-retryably: {reason}",
                        step.id
                    )));
                }
                VerificationVerdict::Fail {
                    reason,
                    retryable: true,
                } => {
                    if attempts >= self.options.max_fix_attempts {
                        return Err(FlowError::FixLoopExhausted(attempts));
                    }
                    attempts = attempts.saturating_add(1);
                    crate::observability::code_intel_metrics::incr_code_edit_fix_attempt();
                    ctx.push(TranscriptEntry::Fix {
                        step_id: step.id.clone(),
                        attempt: attempts,
                        message: reason.clone(),
                    });

                    let mut patched = step.clone();
                    patched.inputs.insert("fix_reason".into(), reason);
                    patched
                        .inputs
                        .insert("attempt".into(), attempts.to_string());
                    outcome = self.executor.execute(ctx, agent, &patched).await?;
                    ctx.push(TranscriptEntry::Exec {
                        step_id: step.id.clone(),
                        artifact: outcome.artifact.clone(),
                    });
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayeredPlan {

    pub layers: Vec<Vec<Step>>,
}

impl LayeredPlan {
    pub fn new(layers: Vec<Vec<Step>>) -> Self {
        Self { layers }
    }

    pub fn total_steps(&self) -> usize {
        self.layers.iter().map(|l| l.len()).sum()
    }
}

impl<P, E, V> PlanExecVerifyFlow<P, E, V>
where
    P: Planner,
    E: Executor,
    V: Verifier,
{

    pub async fn run_layered(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
        plan: LayeredPlan,
    ) -> Result<FlowOutcome, FlowError> {
        if plan.total_steps() == 0 {
            return Ok(FlowOutcome::success(Vec::new(), 0, ctx.clone()));
        }

        let shared_ctx = Arc::new(Mutex::new(ctx.clone()));
        let max_parallel = self.options.max_parallel_per_layer.max(1);
        let mut artifacts: Vec<(usize, Artifact)> = Vec::new();
        let mut total_iterations: u32 = 0;
        let mut step_index_global: usize = 0;

        for (layer_idx, layer) in plan.layers.iter().enumerate() {
            crate::observability::code_intel_metrics::incr_code_edit_parallel_layer_run();
            tracing::info!(
                target: "agent.flows.plan_exec_verify",
                layer = layer_idx,
                step_count = layer.len(),
                max_parallel = max_parallel,
                "layer_dispatch",
            );

            let semaphore = Arc::new(Semaphore::new(max_parallel));
            let mut futs = FuturesUnordered::new();
            let mut layer_indices: Vec<(usize, String)> = Vec::with_capacity(layer.len());

            for step in layer {
                let global_index = step_index_global;
                step_index_global += 1;
                layer_indices.push((global_index, step.id.clone()));
                crate::observability::code_intel_metrics::incr_code_edit_parallel_step_run();

                let permit_sem = semaphore.clone();
                let ctx_handle = shared_ctx.clone();
                let step_owned = step.clone();
                let flow_ref = self;
                let agent_ref = agent;
                futs.push(async move {
                    let _permit = match permit_sem.acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => {
                            return (
                                global_index,
                                step_owned.id.clone(),
                                Err::<(Artifact, u32), FlowError>(FlowError::Other(
                                    "layer semaphore closed".into(),
                                )),
                            );
                        }
                    };

                    let mut local_ctx = ctx_handle.lock().await.clone();
                    let result = flow_ref
                        .execute_step_with_fix(&mut local_ctx, agent_ref, &step_owned)
                        .await;
                    let mut shared = ctx_handle.lock().await;
                    merge_context_deltas(&mut shared, &local_ctx);
                    (global_index, step_owned.id.clone(), result)
                });
            }

            let mut layer_err: Option<FlowError> = None;
            let mut layer_artifacts: Vec<(usize, Artifact)> = Vec::new();
            while let Some((idx, step_id, res)) = futs.next().await {
                match res {
                    Ok((artifact, attempts)) => {
                        total_iterations = total_iterations.saturating_add(attempts);
                        layer_artifacts.push((idx, artifact));
                    }
                    Err(e) => {
                        if layer_err.is_none() {
                            layer_err = Some(FlowError::Executor(format!(
                                "layered step `{step_id}` failed: {e}"
                            )));
                        }

                    }
                }
            }

            if let Some(err) = layer_err {
                let final_ctx = shared_ctx.lock().await.clone();
                *ctx = final_ctx;
                return Err(err);
            }
            artifacts.extend(layer_artifacts);
        }

        artifacts.sort_by_key(|(i, _)| *i);
        let final_ctx = shared_ctx.lock().await.clone();
        *ctx = final_ctx;
        Ok(FlowOutcome::success(
            artifacts.into_iter().map(|(_, a)| a).collect(),
            total_iterations,
            ctx.clone(),
        ))
    }
}

fn merge_context_deltas(shared: &mut FlowContext, local: &FlowContext) {
    let shared_len = shared.transcript.len();
    if local.transcript.len() > shared_len {
        shared
            .transcript
            .extend_from_slice(&local.transcript[shared_len..]);
    }
    let mut additions: HashMap<String, String> = HashMap::new();
    for (k, v) in &local.scratchpad {
        if !shared.scratchpad.contains_key(k) {
            additions.insert(k.clone(), v.clone());
        }
    }
    shared.scratchpad.extend(additions);
}

#[async_trait]
impl<P, E, V> Flow for PlanExecVerifyFlow<P, E, V>
where
    P: Planner,
    E: Executor,
    V: Verifier,
{
    fn name(&self) -> &'static str {
        self.name
    }

    async fn run(
        &self,
        ctx: &mut FlowContext,
        agent: &dyn AgentHandle,
    ) -> Result<FlowOutcome, FlowError> {
        match self.run_once(ctx, agent).await {
            Ok((artifacts, iterations)) => {
                Ok(FlowOutcome::success(artifacts, iterations, ctx.clone()))
            }
            Err(FlowError::FixLoopExhausted(attempts)) if self.options.allow_single_replan => {

                let replan_marker = super::traits::TranscriptEntry::Fix {
                    step_id: "__replan__".into(),
                    attempt: attempts,
                    message: "single replan after fix-loop exhaustion".into(),
                };
                ctx.push(replan_marker);
                let (artifacts, iterations) = self.run_once(ctx, agent).await?;
                Ok(FlowOutcome::success(artifacts, iterations, ctx.clone()))
            }
            Err(e) => Err(e),
        }
    }
}
