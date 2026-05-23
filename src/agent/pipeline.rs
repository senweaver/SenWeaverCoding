// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Debug, Clone)]
pub struct PipelineStage {
    pub name: String,
    pub kind: StageKind,

    pub max_parallel: usize,

    pub tasks: Vec<PipelineTask>,

    pub error_strategy: StageErrorStrategy,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum StageKind {

    Sequential,

    #[default]
    Parallel,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum StageErrorStrategy {

    #[default]
    FailFast,

    CollectAll,

    SkipFailed,
}

#[derive(Debug, Clone)]
pub struct PipelineTask {
    pub id: String,
    pub name: String,

    pub input: serde_json::Value,

    pub from_task: Option<String>,

    pub extract_field: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: String,
    pub output: serde_json::Value,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct StageResult {
    pub stage_name: String,
    pub results: Vec<TaskResult>,
    pub all_success: bool,
}

#[derive(Debug, Clone)]
pub struct PipelineConfig {

    pub max_stage_parallelism: usize,

    pub stage_buffer_size: usize,

    pub total_timeout_secs: Option<u64>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_stage_parallelism: 4,
            stage_buffer_size: 64,
            total_timeout_secs: Some(300),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PipelineResult {

    pub stages: Vec<StageResult>,

    pub total_duration_ms: u64,

    pub success: bool,

    pub output: HashMap<String, serde_json::Value>,
}

#[derive(Default)]
pub struct PipelineBuilder {
    stages: Vec<PipelineStage>,
    config: PipelineConfig,
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(mut self, config: PipelineConfig) -> Self {
        self.config = config;
        self
    }

    pub fn stage_parallel(mut self, name: impl Into<String>, max_parallel: usize) -> StageBuilder {
        let stage = PipelineStage {
            name: name.into(),
            kind: StageKind::Parallel,
            max_parallel,
            tasks: Vec::new(),
            error_strategy: StageErrorStrategy::default(),
        };
        self.stages.push(stage);
        StageBuilder { pipeline: self }
    }

    pub fn stage_seq(mut self, name: impl Into<String>) -> StageBuilder {
        let stage = PipelineStage {
            name: name.into(),
            kind: StageKind::Sequential,
            max_parallel: 1,
            tasks: Vec::new(),
            error_strategy: StageErrorStrategy::default(),
        };
        self.stages.push(stage);
        StageBuilder { pipeline: self }
    }

    pub fn build(self) -> Pipeline {
        Pipeline {
            stages: self.stages,
            config: self.config,
        }
    }
}

#[derive(Default)]
pub struct StageBuilder {
    pipeline: PipelineBuilder,
}

impl StageBuilder {
    pub fn task(mut self, id: impl Into<String>, name: impl Into<String>) -> Self {
        let pipeline = &mut self.pipeline;
        if let Some(stage) = pipeline.stages.last_mut() {
            stage.tasks.push(PipelineTask {
                id: id.into(),
                name: name.into(),
                input: serde_json::Value::Null,
                from_task: None,
                extract_field: None,
            });
        }
        self
    }

    pub fn with_input(mut self, input: serde_json::Value) -> Self {
        let pipeline = &mut self.pipeline;
        if let Some(stage) = pipeline.stages.last_mut() {
            if let Some(task) = stage.tasks.last_mut() {
                task.input = input;
            }
        }
        self
    }

    pub fn from_previous(mut self, task_id: &str, field: &str) -> Self {
        let pipeline = &mut self.pipeline;
        if let Some(stage) = pipeline.stages.last_mut() {
            if let Some(task) = stage.tasks.last_mut() {
                task.from_task = Some(task_id.to_string());
                task.extract_field = Some(field.to_string());
            }
        }
        self
    }

    pub fn error_strategy(mut self, strategy: StageErrorStrategy) -> Self {
        let pipeline = &mut self.pipeline;
        if let Some(stage) = pipeline.stages.last_mut() {
            stage.error_strategy = strategy;
        }
        self
    }

    pub fn stage(self) -> PipelineBuilder {
        self.pipeline
    }
}

#[derive(Debug, Clone)]
pub struct Pipeline {
    pub stages: Vec<PipelineStage>,
    pub config: PipelineConfig,
}

impl Pipeline {

    pub async fn execute<F, Fut>(&self, executor: F) -> PipelineResult
    where
        F: Fn(String, serde_json::Value) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = TaskResult> + Send + 'static,
    {
        let start = std::time::Instant::now();
        let mut stage_results: Vec<StageResult> = Vec::new();
        let mut context: HashMap<String, serde_json::Value> = HashMap::new();
        let semaphore = Arc::new(Semaphore::new(self.config.max_stage_parallelism));

        for stage in &self.stages {
            let result = self
                .execute_stage(stage, executor.clone(), &context, Arc::clone(&semaphore))
                .await;

            if !result.all_success && matches!(stage.error_strategy, StageErrorStrategy::FailFast) {
                tracing::error!("Pipeline failed at stage '{}'", stage.name);
                break;
            }

            for task_result in &result.results {
                context.insert(task_result.task_id.clone(), task_result.output.clone());
            }

            stage_results.push(result);
        }

        let total_ms = start.elapsed().as_millis() as u64;
        PipelineResult {
            stages: stage_results,
            total_duration_ms: total_ms,
            success: true,
            output: context,
        }
    }

    async fn execute_stage<F, Fut>(
        &self,
        stage: &PipelineStage,
        executor: F,
        context: &HashMap<String, serde_json::Value>,
        semaphore: Arc<Semaphore>,
    ) -> StageResult
    where
        F: Fn(String, serde_json::Value) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = TaskResult> + Send + 'static,
    {
        let mut results = Vec::new();

        match stage.kind {
            StageKind::Sequential => {
                for task in &stage.tasks {
                    let input = self.resolve_input(task, context);
                    let result = executor(task.id.clone(), input).await;
                    results.push(result);
                }
            }
            StageKind::Parallel => {
                let mut handles = Vec::new();
                for task in &stage.tasks {
                    let permit = match semaphore.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(
                                target = "agent.pipeline",
                                stage = %stage.name,
                                task = %task.id,
                                error = %e,
                                "pipeline semaphore closed; task failed without execution"
                            );
                            results.push(TaskResult {
                                task_id: task.id.clone(),
                                output: serde_json::Value::Null,
                                success: false,
                                error: Some(format!(
                                    "pipeline semaphore closed before acquire: {e}"
                                )),
                                duration_ms: 0,
                            });
                            continue;
                        }
                    };
                    let executor = executor.clone();
                    let input = self.resolve_input(task, context);
                    let task_id = task.id.clone();
                    let handle = tokio::spawn(async move {
                        let result = executor(task_id, input).await;
                        drop(permit);
                        result
                    });
                    handles.push(handle);
                }
                for handle in handles {
                    if let Ok(result) = handle.await {
                        results.push(result);
                    }
                }
            }
        }

        let all_success = results.iter().all(|r| r.success);
        StageResult {
            stage_name: stage.name.clone(),
            results,
            all_success,
        }
    }

    fn resolve_input(
        &self,
        task: &PipelineTask,
        context: &HashMap<String, serde_json::Value>,
    ) -> serde_json::Value {

        if let (Some(src_task), Some(field)) = (&task.from_task, &task.extract_field) {
            if let Some(src_output) = context.get(src_task) {
                if let Some(extracted) = src_output.get(field) {
                    return extracted.clone();
                }
            }
        }
        task.input.clone()
    }
}
