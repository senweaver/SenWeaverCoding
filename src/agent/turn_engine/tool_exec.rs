// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use async_trait::async_trait;
use futures_util::future::join_all;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

use crate::tools::{Tool, ToolResult};

pub fn tool_fingerprint(tool: &dyn Tool, args: &Value) -> Option<String> {
    tool.fingerprint(args)
}

pub fn tool_cache_ttl_secs(tool: &dyn Tool) -> u64 {
    tool.cache_ttl_secs()
}

#[derive(Debug, Clone)]
pub struct ParallelToolCall {
    pub name: String,
    pub args: Value,

    pub simulated_latency: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct ParallelToolOutcome {
    pub name: String,
    pub result: ToolResult,
}

#[async_trait]
pub trait ParallelToolExec: Send + Sync {
    async fn run(
        &self,
        tools: &[Arc<dyn Tool>],
        calls: Vec<ParallelToolCall>,
    ) -> Vec<ParallelToolOutcome>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JoinAllExec;

#[async_trait]
impl ParallelToolExec for JoinAllExec {
    async fn run(
        &self,
        tools: &[Arc<dyn Tool>],
        calls: Vec<ParallelToolCall>,
    ) -> Vec<ParallelToolOutcome> {
        let futs: Vec<_> = calls
            .into_iter()
            .map(|call| {
                let tool_opt = tools.iter().find(|t| t.name() == call.name).cloned();
                async move {
                    if let Some(delay) = call.simulated_latency {
                        tokio::time::sleep(delay).await;
                    }
                    let result = match tool_opt {
                        Some(tool) => {
                            tool.execute(call.args)
                                .await
                                .unwrap_or_else(|e| ToolResult {
                                    output: String::new(),
                                    success: false,
                                    error: Some(e.to_string()),
                                })
                        }
                        None => ToolResult {
                            output: String::new(),
                            success: false,
                            error: Some(format!("tool '{}' not found", call.name)),
                        },
                    };
                    ParallelToolOutcome {
                        name: call.name,
                        result,
                    }
                }
            })
            .collect();

        join_all(futs).await
    }
}
