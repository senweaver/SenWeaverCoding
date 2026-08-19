// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::time::{Duration, timeout};
use tracing::{debug, error, info, trace, warn};

use crate::workflows::types::{
    ErrorMode, StepAgent, StepMode, StepResult, Workflow, WorkflowRun, WorkflowStep,
    validate_workflow,
};

#[derive(Debug, Clone, Copy)]
pub struct WorkflowEngine;

impl WorkflowEngine {

    pub fn new() -> Self {
        Self
    }

    pub async fn execute_run<F, Fut>(
        &self,
        workflow: &Workflow,
        mut run: WorkflowRun,
        agent_resolver: impl Fn(&StepAgent) -> Option<(String, String)> + Send + Sync + 'static,
        step_executor: F,
    ) -> WorkflowRun
    where
        F: Fn(StepAgent, String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(String, u64, u64), String>> + Send + 'static,
    {

        if let Err(e) = validate_workflow(workflow) {
            run.mark_failed(format!("Workflow validation failed: {}", e));
            return run;
        }

        for (key, value) in &workflow.variables {
            run.variables
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }

        run.mark_started();
        info!(
            run_id = %run.id,
            workflow_id = %workflow.id,
            step_count = workflow.steps.len(),
            "Starting workflow execution"
        );

        let workflow_timeout = Duration::from_secs(workflow.timeout_secs);

        let boxed_executor: Arc<
            dyn Fn(
                    StepAgent,
                    String,
                )
                    -> Pin<Box<dyn Future<Output = Result<(String, u64, u64), String>> + Send>>
                + Send
                + Sync,
        > = Arc::new(move |agent, prompt| Box::pin(step_executor(agent, prompt)));

        let execution = async {
            self.execute_steps(workflow, &mut run, agent_resolver, boxed_executor)
                .await
        };

        match timeout(workflow_timeout, execution).await {
            Ok(result) => result,
            Err(_) => {
                let msg = format!("Workflow timeout after {} seconds", workflow.timeout_secs);
                error!(run_id = %run.id, "{}", msg);
                run.mark_failed(msg);
                run
            }
        }
    }

    async fn execute_steps(
        &self,
        workflow: &Workflow,
        run: &mut WorkflowRun,
        agent_resolver: impl Fn(&StepAgent) -> Option<(String, String)> + Send + Sync,
        step_executor: Arc<
            dyn Fn(
                    StepAgent,
                    String,
                )
                    -> Pin<Box<dyn Future<Output = Result<(String, u64, u64), String>> + Send>>
                + Send
                + Sync,
        >,
    ) -> WorkflowRun {
        let steps = workflow.steps.clone();
        let mut current_input = run.input.clone();
        let mut fanout_outputs: Vec<String> = Vec::new();
        let mut i = 0;

        while i < steps.len() {
            let step = &steps[i];

            if let Some((resolved_id, resolved_name)) = agent_resolver(&step.agent) {
                debug!(
                    run_id = %run.id,
                    step = %step.name,
                    agent_id = %resolved_id,
                    agent_name = %resolved_name,
                    "Resolved agent for step"
                );
            }

            trace!(
                run_id = %run.id,
                step = %step.name,
                index = i,
                mode = ?step.mode,
                "Executing step"
            );

            match &step.mode {
                StepMode::Sequential => {
                    let result = self
                        .execute_step_with_error_mode(
                            step,
                            i,
                            &current_input,
                            &run.variables,
                            &step_executor,
                        )
                        .await;

                    if !result.success {
                        run.add_step_result(result.clone());
                        if !step.error_mode.allows_continue() {
                            run.mark_failed(
                                result.error.unwrap_or_else(|| "Step failed".to_string()),
                            );
                            return run.clone();
                        }

                        i += 1;
                        continue;
                    }

                    current_input = result.output.clone();
                    run.add_step_result(result);

                    if let Some(var_name) = &step.output_var {
                        run.set_variable(var_name.clone(), current_input.clone());
                        debug!(
                            run_id = %run.id,
                            variable = %var_name,
                            "Set workflow variable"
                        );
                    }

                    i += 1;
                }

                StepMode::FanOut => {

                    let fanout_start = i;
                    let mut fanout_steps = vec![step.clone()];

                    while i + 1 < steps.len() && matches!(steps[i + 1].mode, StepMode::FanOut) {
                        i += 1;
                        fanout_steps.push(steps[i].clone());
                    }

                    info!(
                        run_id = %run.id,
                        fanout_count = fanout_steps.len(),
                        "Executing FanOut steps in parallel"
                    );

                    let mut fanout_futures = Vec::new();
                    for (idx, step) in fanout_steps.iter().enumerate() {
                        let step = step.clone();
                        let input = current_input.clone();
                        let vars = run.variables.clone();
                        let executor_clone = step_executor.clone();
                        let engine = *self;
                        let future = async move {
                            let result = engine
                                .execute_step_with_error_mode(
                                    &step,
                                    fanout_start + idx,
                                    &input,
                                    &vars,
                                    &executor_clone,
                                )
                                .await;
                            (step.name.clone(), result)
                        };
                        fanout_futures.push(future);
                    }

                    let results = if fanout_futures.len() == 1 {
                        match fanout_futures.into_iter().next() {
                            Some(fut) => vec![fut.await],
                            None => Vec::new(),
                        }
                    } else {
                        let mut receivers = Vec::new();
                        for (idx, fut) in fanout_futures.into_iter().enumerate() {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            crate::runtime::spawn_supervised(
                                format!("workflows.fanout.{}", idx),
                                async move {
                                    let _ = tx.send(fut.await);
                                },
                            );
                            receivers.push((idx, rx));
                        }
                        let mut collected = Vec::new();
                        for (idx, rx) in receivers {
                            match rx.await {
                                Ok(r) => collected.push(r),
                                Err(_) => {
                                    let step_name = fanout_steps
                                        .get(idx)
                                        .map(|s| s.name.clone())
                                        .unwrap_or_else(|| format!("fanout-{idx}"));
                                    warn!(
                                        run_id = %run.id,
                                        step = %step_name,
                                        "FanOut step task aborted before reporting a result \
                                         (likely panicked); recording as failed"
                                    );
                                    collected.push((
                                        step_name.clone(),
                                        crate::workflows::types::StepResult::failure(
                                            step_name,
                                            fanout_start + idx,
                                            "step task aborted before completion",
                                        ),
                                    ));
                                }
                            }
                        }
                        collected
                    };

                    fanout_outputs.clear();
                    let mut any_failed = false;
                    for (name, result) in results {
                        if result.success {
                            fanout_outputs.push(result.output.clone());
                        } else {
                            any_failed = true;
                            warn!(
                                run_id = %run.id,
                                step = %name,
                                "FanOut step failed: {}",
                                result.error.as_ref().unwrap_or(&"Unknown error".to_string())
                            );
                        }
                        run.add_step_result(result);
                    }

                    if any_failed && fanout_steps.iter().any(|s| s.error_mode == ErrorMode::Fail) {
                        run.mark_failed("One or more FanOut steps failed");
                        return run.clone();
                    }

                    i += 1;
                }

                StepMode::Collect => {
                    info!(
                        run_id = %run.id,
                        "Collecting {} FanOut outputs",
                        fanout_outputs.len()
                    );

                    current_input = fanout_outputs.join("\n\n---\n\n");
                    fanout_outputs.clear();

                    let result = self
                        .execute_step_with_error_mode(
                            step,
                            i,
                            &current_input,
                            &run.variables,
                            &step_executor,
                        )
                        .await;

                    if !result.success {
                        run.add_step_result(result.clone());
                        if !step.error_mode.allows_continue() {
                            run.mark_failed(
                                result
                                    .error
                                    .unwrap_or_else(|| "Collect step failed".to_string()),
                            );
                            return run.clone();
                        }
                    } else {
                        current_input = result.output.clone();
                        run.add_step_result(result);

                        if let Some(var_name) = &step.output_var {
                            run.set_variable(var_name.clone(), current_input.clone());
                        }
                    }

                    i += 1;
                }

                StepMode::Conditional { condition } => {

                    let condition_met = evaluate_condition(condition, &current_input);

                    debug!(
                        run_id = %run.id,
                        step = %step.name,
                        condition = %condition,
                        met = condition_met,
                        "Evaluated condition"
                    );

                    if condition_met {

                        let result = self
                            .execute_step_with_error_mode(
                                step,
                                i,
                                &current_input,
                                &run.variables,
                                &step_executor,
                            )
                            .await;

                        if !result.success {
                            run.add_step_result(result.clone());
                            if !step.error_mode.allows_continue() {
                                run.mark_failed(
                                    result
                                        .error
                                        .unwrap_or_else(|| "Conditional step failed".to_string()),
                                );
                                return run.clone();
                            }
                        } else {
                            current_input = result.output.clone();
                            run.add_step_result(result);

                            if let Some(var_name) = &step.output_var {
                                run.set_variable(var_name.clone(), current_input.clone());
                            }
                        }
                    } else {

                        let skipped_result = StepResult {
                            step_name: step.name.clone(),
                            step_index: i,
                            success: true,
                            output: "(skipped - condition not met)".to_string(),
                            token_usage: None,
                            duration_ms: 0,
                            error: None,
                        };
                        run.add_step_result(skipped_result);
                    }

                    i += 1;
                }

                StepMode::Loop {
                    max_iterations,
                    until,
                } => {
                    let mut iteration = 0;
                    let mut loop_output = current_input.clone();
                    let max_iter = *max_iterations;
                    let until_str = until.clone();

                    loop {

                        let result = self
                            .execute_step_with_error_mode(
                                step,
                                i,
                                &loop_output,
                                &run.variables,
                                &step_executor,
                            )
                            .await;

                        if !result.success {
                            run.add_step_result(result.clone());
                            if !step.error_mode.allows_continue() {
                                run.mark_failed(
                                    result
                                        .error
                                        .unwrap_or_else(|| "Loop step failed".to_string()),
                                );
                                return run.clone();
                            }
                            break;
                        }

                        loop_output = result.output.clone();
                        run.add_step_result(result);

                        if !until_str.is_empty()
                            && loop_output
                                .to_lowercase()
                                .contains(&until_str.to_lowercase())
                        {
                            debug!(
                                run_id = %run.id,
                                step = %step.name,
                                iteration,
                                "Loop exit condition met"
                            );
                            break;
                        }

                        iteration += 1;
                        if iteration >= max_iter {
                            break;
                        }
                    }

                    current_input = loop_output;

                    if let Some(var_name) = &step.output_var {
                        run.set_variable(var_name.clone(), current_input.clone());
                    }

                    i += 1;
                }
            }
        }

        run.mark_completed(current_input);
        info!(
            run_id = %run.id,
            duration_secs = run.duration_secs(),
            "Workflow completed successfully"
        );

        run.clone()
    }

    async fn execute_step_with_error_mode(
        &self,
        step: &WorkflowStep,
        index: usize,
        input: &str,
        variables: &HashMap<String, String>,
        step_executor: &Arc<
            dyn Fn(
                    StepAgent,
                    String,
                )
                    -> Pin<Box<dyn Future<Output = Result<(String, u64, u64), String>> + Send>>
                + Send
                + Sync,
        >,
    ) -> StepResult {
        let expanded_prompt = step.expand_prompt(variables, input);

        let max_retries = step.error_mode.max_retries();
        let mut last_error = None;

        for attempt in 0..=max_retries {
            if attempt > 0 {
                debug!(
                    step = %step.name,
                    attempt,
                    "Retrying step"
                );
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }

            let result = self
                .execute_single_step(step, index, &expanded_prompt, step_executor)
                .await;

            if result.success {
                return result;
            }

            last_error = result.error.clone();

            if attempt == max_retries {

                return StepResult::failure(
                    &step.name,
                    index,
                    last_error.unwrap_or_else(|| "Step failed".to_string()),
                );
            }
        }

        StepResult::failure(
            &step.name,
            index,
            last_error.unwrap_or_else(|| "Step failed".to_string()),
        )
    }

    async fn execute_single_step(
        &self,
        step: &WorkflowStep,
        index: usize,
        prompt: &str,
        step_executor: &Arc<
            dyn Fn(
                    StepAgent,
                    String,
                )
                    -> Pin<Box<dyn Future<Output = Result<(String, u64, u64), String>> + Send>>
                + Send
                + Sync,
        >,
    ) -> StepResult {
        let start = Instant::now();

        let step_timeout = Duration::from_secs(step.timeout_secs);
        let execution = step_executor(step.agent.clone(), prompt.to_string());

        match timeout(step_timeout, execution).await {
            Ok(Ok((output, input_tokens, output_tokens))) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                StepResult::success(&step.name, index, output)
                    .with_tokens(input_tokens, output_tokens)
                    .with_duration(duration_ms)
            }
            Ok(Err(e)) => StepResult::failure(&step.name, index, e),
            Err(_) => StepResult::failure(
                &step.name,
                index,
                format!("Step timeout after {} seconds", step.timeout_secs),
            ),
        }
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn evaluate_condition(condition: &str, input: &str) -> bool {
    let input_lower = input.to_lowercase();
    let condition_lower = condition.to_lowercase();

    if condition_lower.starts_with("contains:") {
        let search = &condition_lower[9..];
        return input_lower.contains(search);
    }

    match condition_lower.as_str() {
        "empty" => input.is_empty(),
        "not_empty" => !input.is_empty(),
        "true" => true,
        "false" => false,
        _ => {

            input_lower.contains(&condition_lower)
        }
    }
}
