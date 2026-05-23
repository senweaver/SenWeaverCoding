// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::agent::scheduler::{SchedulableTask, TaskScheduler};
use crate::coordinator::delegation::MergeStrategy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTaskSpec {
    pub id: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "default_capability")]
    pub capability: String,
}

fn default_capability() -> String {
    "general".to_string()
}

fn default_max_parallel() -> usize {
    4
}

fn default_merge_strategy() -> MergeStrategy {
    MergeStrategy::All
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
    #[serde(default = "default_merge_strategy")]
    pub merge_strategy: MergeStrategy,
    pub tasks: Vec<WorkflowTaskSpec>,
}

impl WorkflowSpec {

    pub fn from_json_str(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| format!("invalid JSON: {e}"))
    }

    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "json" => Self::from_json_str(&content),
            "yaml" | "yml" => {

                Err(format!(
                    "YAML is not yet supported directly (file: {}). Convert to JSON \
                     using `yq -o=json .` and rerun.",
                    path.display()
                ))
            }
            _ => Err(format!(
                "unknown workflow file extension '{ext}' — expected .json"
            )),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("workflow.name is empty".into());
        }
        if self.tasks.is_empty() {
            return Err("workflow.tasks is empty".into());
        }
        if self.max_parallel == 0 {
            return Err("workflow.max_parallel must be >= 1".into());
        }
        use std::collections::HashSet;
        let ids: HashSet<&str> = self.tasks.iter().map(|t| t.id.as_str()).collect();
        if ids.len() != self.tasks.len() {
            return Err("duplicate task ids in workflow".into());
        }
        for task in &self.tasks {
            if task.id.trim().is_empty() {
                return Err("a task has an empty id".into());
            }
            if task.prompt.trim().is_empty() {
                return Err(format!("task '{}' has an empty prompt", task.id));
            }
            for dep in &task.depends_on {
                if !ids.contains(dep.as_str()) {
                    return Err(format!("task '{}' depends on unknown '{}'", task.id, dep));
                }
            }
        }
        Ok(())
    }

    pub fn build_scheduler(&self) -> Result<TaskScheduler, String> {
        self.validate()?;
        let mut scheduler = TaskScheduler::new(self.max_parallel);
        let schedulable: Vec<SchedulableTask> = self
            .tasks
            .iter()
            .map(|t| {
                let mut s =
                    SchedulableTask::new(t.id.clone(), t.description.clone(), t.prompt.clone());
                s.required_capability = t.capability.clone();
                for dep in &t.depends_on {
                    s = s.with_dependency(dep.clone());
                }
                s
            })
            .collect();
        scheduler.add_tasks(schedulable)?;
        Ok(scheduler)
    }
}
