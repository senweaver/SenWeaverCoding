// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Workflow Executor  - Execute workflow runs with step dispatch.
//!
//! This module provides the core workflow execution engine that:
//! - Executes workflow steps sequentially or in parallel
//! - Handles FanOut/Collect patterns for parallel execution
//! - Manages conditional branching and loop iteration
//! - Handles errors according to ErrorMode

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
// WorkflowRunStatus is re-exported from mod.rs for external consumers.

/// Workflow execution engine.
#[derive(Debug, Clone, Copy)]
pub struct WorkflowEngine;

impl WorkflowEngine {
    /// Create a new workflow engine.
    pub fn new() -> Self {
        Self
    }

    /// Execute a workflow run.
    ///
    /// This is the main entry point for workflow execution. It takes:
    /// - The workflow definition
    /// - The run state
    /// - An agent resolver (to find agent IDs/names)
    /// - A step executor (to actually run agent steps)
    ///
    /// Returns the completed run (success or failure).
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
        // Validate workflow before execution
        if let Err(e) = validate_workflow(workflow) {
            run.mark_failed(format!("Workflow validation failed: {}", e));
            return run;
        }

        // Seed run variables from workflow defaults (caller-provided values take precedence)
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

    /// Execute workflow steps.
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
                        // If Skip, continue to next step with same input
                        i += 1;
                        continue;
                    }

                    current_input = result.output.clone();
                    run.add_step_result(result);

                    // Store output variable if specified
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
                    // Collect consecutive FanOut steps
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

                    // Execute all FanOut steps in parallel
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

                    // Execute all in parallel using tokio::spawn
                    let results = if fanout_futures.len() == 1 {
                        vec![fanout_futures.into_iter().next().unwrap().await]
                    } else {
                        // For multiple futures, execute them and collect
                        let mut handles = Vec::new();
                        for fut in fanout_futures {
                            handles.push(tokio::spawn(fut));
                        }
                        let mut collected = Vec::new();
                        for handle in handles {
                            if let Ok(r) = handle.await {
                                collected.push(r);
                            }
                        }
                        collected
                    };

                    // Collect outputs and add results
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

                    // If any FanOut step in this batch failed and we're in Fail mode, stop
                    if any_failed && fanout_steps.iter().any(|s| s.error_mode == ErrorMode::Fail) {
                        run.mark_failed("One or more FanOut steps failed");
                        return run.clone();
                    }

                    // Keep current input for next sequential step
                    i += 1;
                }

                StepMode::Collect => {
                    info!(
                        run_id = %run.id,
                        "Collecting {} FanOut outputs",
                        fanout_outputs.len()
                    );

                    // Merge all fanout outputs
                    current_input = fanout_outputs.join("\n\n---\n\n");
                    fanout_outputs.clear();

                    // Execute the Collect step with merged input
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
                    // Evaluate condition against current input
                    let condition_met = evaluate_condition(condition, &current_input);

                    debug!(
                        run_id = %run.id,
                        step = %step.name,
                        condition = %condition,
                        met = condition_met,
                        "Evaluated condition"
                    );

                    if condition_met {
                        // Execute the conditional step
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
                        // Skip this step (add a skipped result)
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
                        // Execute step
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

                        // Check exit condition AFTER execution (do-while semantics)
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

        // Workflow completed successfully
        run.mark_completed(current_input);
        info!(
            run_id = %run.id,
            duration_secs = run.duration_secs(),
            "Workflow completed successfully"
        );

        run.clone()
    }

    /// Execute a single step with error mode handling.
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
                // All retries exhausted
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

    /// Execute a single step (one attempt).
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

        // Execute with timeout
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

/// Evaluate a simple condition string against input.
///
/// Supports:
/// - "contains:X" - input contains X
/// - "empty" - input is empty
/// - "not_empty" - input is not empty
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
            // Default: check if condition string is in input
            input_lower.contains(&condition_lower)
        }
    }
}

/// Create a simple step executor for testing.
pub fn mock_step_executor()
-> impl Fn(StepAgent, String) -> std::future::Ready<Result<(String, u64, u64), String>> {
    |_agent, prompt| {
        std::future::ready(Ok((
            format!("Processed: {}", prompt),
            prompt.len() as u64 / 4,
            100,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::types::{
        ErrorMode, StepAgent, StepMode, Workflow, WorkflowRunStatus, WorkflowStep,
    };

    #[tokio::test]
    async fn test_execute_simple_workflow() {
        let engine = WorkflowEngine::new();

        let workflow =
            Workflow::new("Test").add_step(WorkflowStep::new("step1", "Process {{input}}"));

        let run = WorkflowRun::new(workflow.id.clone(), "hello");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        let completed_run = engine
            .execute_run(&workflow, run, resolver, mock_step_executor())
            .await;

        assert!(completed_run.is_terminal());
        assert!(completed_run.status == WorkflowRunStatus::Completed);
        assert_eq!(completed_run.step_results.len(), 1);
    }

    #[tokio::test]
    async fn test_execute_fanout_workflow() {
        let engine = WorkflowEngine::new();

        let workflow = Workflow::new("Fanout Test")
            .add_step(WorkflowStep::new("fan1", "Analyze {{input}}").with_mode(StepMode::FanOut))
            .add_step(WorkflowStep::new("fan2", "Check {{input}}").with_mode(StepMode::FanOut))
            .add_step(WorkflowStep::new("collect", "Summarize").with_mode(StepMode::Collect));

        let run = WorkflowRun::new(workflow.id.clone(), "data");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        let completed_run = engine
            .execute_run(&workflow, run, resolver, mock_step_executor())
            .await;

        assert!(completed_run.status == WorkflowRunStatus::Completed);
        assert_eq!(completed_run.step_results.len(), 3);
    }

    #[tokio::test]
    async fn test_execute_conditional_step() {
        let engine = WorkflowEngine::new();

        // Step that should execute because input contains "test"
        let workflow = Workflow::new("Conditional").add_step(
            WorkflowStep::new("check", "Process {{input}}").with_mode(StepMode::Conditional {
                condition: "contains:test".to_string(),
            }),
        );

        let run = WorkflowRun::new(workflow.id.clone(), "this is a test");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        let completed_run = engine
            .execute_run(&workflow, run, resolver, mock_step_executor())
            .await;

        assert!(completed_run.status == WorkflowRunStatus::Completed);
        // Should have executed the conditional step
        assert_eq!(completed_run.step_results.len(), 1);
        assert!(completed_run.step_results[0].success);
    }

    #[tokio::test]
    async fn test_execute_skipped_conditional() {
        let engine = WorkflowEngine::new();

        // Step that should be skipped because condition not met
        let workflow = Workflow::new("Conditional").add_step(
            WorkflowStep::new("check", "Process {{input}}").with_mode(StepMode::Conditional {
                condition: "contains:xyz".to_string(),
            }),
        );

        let run = WorkflowRun::new(workflow.id.clone(), "this is a test");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        let completed_run = engine
            .execute_run(&workflow, run, resolver, mock_step_executor())
            .await;

        assert!(completed_run.status == WorkflowRunStatus::Completed);
        // Step should be "skipped" but still counted
        assert_eq!(completed_run.step_results.len(), 1);
        assert!(completed_run.step_results[0].output.contains("skipped"));
    }

    #[tokio::test]
    async fn test_execute_loop_step() {
        let engine = WorkflowEngine::new();

        // Loop that should exit after a few iterations when output contains "done"
        let workflow = Workflow::new("Loop Test").add_step(
            WorkflowStep::new("iterate", "Process {{input}} done").with_mode(StepMode::Loop {
                max_iterations: 3,
                until: "done".to_string(),
            }),
        );

        let run = WorkflowRun::new(workflow.id.clone(), "start");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        let completed_run = engine
            .execute_run(&workflow, run, resolver, mock_step_executor())
            .await;

        assert!(completed_run.status == WorkflowRunStatus::Completed);
        // Should have multiple step results (one per iteration)
        assert!(completed_run.step_results.len() > 0);
    }

    #[tokio::test]
    async fn test_workflow_timeout() {
        let engine = WorkflowEngine::new();

        let workflow = Workflow::new("Slow Workflow")
            .with_timeout(1) // 1 second timeout
            .add_step(WorkflowStep::new("slow", "Take too long"));

        let run = WorkflowRun::new(workflow.id.clone(), "input");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        // Create a slow executor
        let slow_executor = |_agent: StepAgent, _prompt: String| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(("done".to_string(), 10, 10))
        };

        let completed_run = engine
            .execute_run(&workflow, run, resolver, slow_executor)
            .await;

        assert!(completed_run.status == WorkflowRunStatus::Failed);
        assert!(completed_run.error.as_ref().unwrap().contains("timeout"));
    }

    #[tokio::test]
    async fn test_error_with_retry() {
        let engine = WorkflowEngine::new();

        let workflow = Workflow::new("Retry Test").add_step(
            WorkflowStep::new("flaky", "Do something")
                .with_error_mode(ErrorMode::Retry { max_retries: 2 }),
        );

        let run = WorkflowRun::new(workflow.id.clone(), "input");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        // Create an executor that fails then succeeds (counter in Arc so the closure is `Fn`).
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let flaky_executor = {
            let call_count = std::sync::Arc::clone(&call_count);
            move |_agent: StepAgent, _prompt: String| {
                let n = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let result = if n >= 2 {
                    Ok(("success".to_string(), 10, 10))
                } else {
                    Err("Temporary failure".to_string())
                };
                async move { result }
            }
        };

        let completed_run = engine
            .execute_run(&workflow, run, resolver, flaky_executor)
            .await;

        // After retries, it should eventually succeed
        assert!(completed_run.status == WorkflowRunStatus::Completed);
    }

    #[test]
    fn test_evaluate_condition() {
        assert!(evaluate_condition("contains:test", "this is a test"));
        assert!(!evaluate_condition("contains:xyz", "this is a test"));
        assert!(evaluate_condition("empty", ""));
        assert!(!evaluate_condition("empty", "not empty"));
        assert!(evaluate_condition("not_empty", "something"));
        assert!(!evaluate_condition("not_empty", ""));
        assert!(evaluate_condition("true", ""));
        assert!(!evaluate_condition("false", ""));
    }

    #[tokio::test]
    async fn test_workflow_validation_failure() {
        let engine = WorkflowEngine::new();

        // Empty workflow (no steps)
        let workflow = Workflow::new("Empty");
        let run = WorkflowRun::new(workflow.id.clone(), "input");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        let completed_run = engine
            .execute_run(&workflow, run, resolver, mock_step_executor())
            .await;

        assert!(completed_run.status == WorkflowRunStatus::Failed);
        assert!(completed_run.error.as_ref().unwrap().contains("validation"));
    }

    #[tokio::test]
    async fn test_step_output_variable() {
        let engine = WorkflowEngine::new();

        let workflow = Workflow::new("Variable Test")
            .add_step(WorkflowStep::new("step1", "Process {{input}}").with_output_var("result"));

        let run = WorkflowRun::new(workflow.id.clone(), "hello");
        let resolver = |_agent: &StepAgent| Some(("agent-1".to_string(), "Test Agent".to_string()));

        let completed_run = engine
            .execute_run(&workflow, run, resolver, mock_step_executor())
            .await;

        assert!(completed_run.variables.contains_key("result"));
    }
}
