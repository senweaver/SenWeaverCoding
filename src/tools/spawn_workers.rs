// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::traits::{Tool, ToolResult};
use crate::agent::loop_::{DraftEvent, current_tool_call_id, take_parent_draft_channel};
use crate::config::Config;
use crate::config::live::LiveConfig;
use crate::session::current_session_context;
use crate::workers::events::{WorkerResult, WorkerSpec, WorkerStatus};
use crate::workers::runner::WorkerRunContext;
use crate::workers::supervisor::ensure_supervisor;

const TOOL_NAME: &str = "spawn_workers";

#[derive(Debug, Deserialize)]
struct SpawnArgs {
    #[serde(default)]
    tasks: Vec<TaskArgs>,

    #[serde(default = "default_merge_strategy")]
    merge_strategy: MergeStrategy,
}

#[derive(Debug, Deserialize)]
struct TaskArgs {
    #[serde(default)]
    title: Option<String>,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    context: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum MergeStrategy {
    Concat,
    Summary,
}

fn default_merge_strategy() -> MergeStrategy {
    MergeStrategy::Concat
}

pub struct SpawnWorkersTool {
    config: Arc<Config>,
    live_config: Option<LiveConfig>,
}

impl SpawnWorkersTool {
    pub fn new(config: Arc<Config>, live_config: Option<LiveConfig>) -> Self {
        Self { config, live_config }
    }
}

#[async_trait]
impl Tool for SpawnWorkersTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        "Run multiple long-running subtasks as parallel worker sub-agents. Each worker has its own \
         session and detail tab in the UI. Use when you need to decompose a complex request into \
         independent tasks that can run concurrently (e.g. analysing multiple codebases, fanning \
         out research, executing several long jobs). The tool blocks until every worker reaches a \
         terminal state and returns an aggregated result. Workers cannot spawn additional workers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "tasks": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["prompt"],
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Short human-readable label shown in the worker UI."
                            },
                            "prompt": {
                                "type": "string",
                                "minLength": 1,
                                "description": "Full natural-language task for this worker."
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional model override; defaults to the parent session's model."
                            },
                            "context": {
                                "type": "string",
                                "description": "Optional context prepended to the worker's prompt."
                            }
                        }
                    }
                },
                "merge_strategy": {
                    "type": "string",
                    "enum": ["concat", "summary"],
                    "default": "concat",
                    "description": "How to aggregate worker outputs back to the parent: 'concat' joins outputs with separators; 'summary' produces a short per-worker summary."
                }
            },
            "required": ["tasks"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let parsed: SpawnArgs = match serde_json::from_value(args) {
            Ok(p) => p,
            Err(err) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("invalid arguments: {err}")),
                });
            }
        };

        if parsed.tasks.is_empty() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("`tasks` must contain at least one entry".to_string()),
            });
        }

        let parent_session_id = current_session_context()
            .map(|c| c.session_id)
            .unwrap_or_default();
        let parent_tool_use_id = current_tool_call_id().unwrap_or_default();

        let supervisor = match ensure_supervisor() {
            Ok(s) => s,
            Err(err) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(err.to_string()),
                });
            }
        };

        let parent_draft = take_parent_draft_channel();

        let run_ctx = WorkerRunContext {
            config: Arc::clone(&self.config),
            live_config: self.live_config.clone(),
        };

        let mut handles = Vec::with_capacity(parsed.tasks.len());
        for task in &parsed.tasks {
            let title = task
                .title
                .clone()
                .unwrap_or_else(|| derive_title(&task.prompt));
            let spec = WorkerSpec {
                parent_session_id: parent_session_id.clone(),
                parent_tool_use_id: parent_tool_use_id.clone(),
                title,
                prompt: task.prompt.clone(),
                context: task.context.clone(),
                model: task.model.clone(),
            };
            let handle = supervisor.spawn(spec, parent_draft.clone(), run_ctx.clone());
            handles.push(handle);
        }

        let mut waits = Vec::with_capacity(handles.len());
        for h in &handles {
            waits.push(h.wait());
        }

        let mut results: Vec<WorkerResult> = Vec::with_capacity(waits.len());
        for (idx, w) in waits.into_iter().enumerate() {
            match w.await {
                Ok(res) => results.push(res),
                Err(_) => {
                    let h = &handles[idx];
                    results.push(WorkerResult {
                        worker_id: h.worker_id.clone(),
                        title: h.title.clone(),
                        status: WorkerStatus::Failed,
                        output: String::new(),
                        error: Some("worker future dropped before completion".to_string()),
                        started_at: h.started_at,
                        finished_at: h.finished_at(),
                    });
                }
            }
        }

        if let Some(tx) = parent_draft.as_ref() {
            let _ = tx
                .send(DraftEvent::ParentResumed {
                    reason: format!("workers_completed({})", results.len()),
                })
                .await;
        }

        let aggregated = aggregate_results(&results, parsed.merge_strategy);
        let any_failed = results
            .iter()
            .any(|r| matches!(r.status, WorkerStatus::Failed | WorkerStatus::Stopped));

        Ok(ToolResult {
            success: !any_failed,
            output: aggregated,
            error: None,
        })
    }
}

fn derive_title(prompt: &str) -> String {
    let mut iter = prompt.split_whitespace();
    let mut buf = String::new();
    for _ in 0..6 {
        match iter.next() {
            Some(word) => {
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str(word);
            }
            None => break,
        }
    }
    if buf.chars().count() > 60 {
        buf = buf.chars().take(60).collect();
        buf.push('…');
    }
    if buf.is_empty() {
        "worker".to_string()
    } else {
        buf
    }
}

fn aggregate_results(results: &[WorkerResult], strategy: MergeStrategy) -> String {
    match strategy {
        MergeStrategy::Concat => results
            .iter()
            .map(|r| {
                let header = format!(
                    "## {} ({}) [{}]",
                    r.title,
                    r.worker_id,
                    r.status.as_str()
                );
                let body = if r.output.trim().is_empty() {
                    r.error
                        .clone()
                        .unwrap_or_else(|| "<no output>".to_string())
                } else {
                    r.output.clone()
                };
                format!("{header}\n{body}")
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n"),
        MergeStrategy::Summary => {
            let mut out = String::from("Worker results summary:\n");
            for r in results {
                let snippet = if r.output.trim().is_empty() {
                    r.error
                        .clone()
                        .unwrap_or_else(|| "<no output>".to_string())
                } else {
                    truncate(&r.output, 240)
                };
                out.push_str(&format!(
                    "- {} ({}): {} — {}\n",
                    r.title,
                    r.worker_id,
                    r.status.as_str(),
                    snippet
                ));
            }
            out
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.replace('\n', " ");
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out.replace('\n', " ")
}
