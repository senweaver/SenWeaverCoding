// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

use crate::hooks::traits::HookHandler;
use crate::tools::traits::ToolResult;

pub struct CommandLoggerHook {
    log: Arc<Mutex<Vec<String>>>,
}

impl CommandLoggerHook {
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

}

#[async_trait]
impl HookHandler for CommandLoggerHook {
    fn name(&self) -> &str {
        "command-logger"
    }

    fn priority(&self) -> i32 {
        -50
    }

    async fn on_after_tool_call(&self, tool: &str, result: &ToolResult, duration: Duration) {
        let entry = format!(
            "[{}] {} ({}ms) success={}",
            chrono::Utc::now().format("%H:%M:%S"),
            tool,
            duration.as_millis(),
            result.success,
        );
        tracing::info!(hook = "command-logger", "{}", entry);
        self.log.lock().push(entry);
    }
}
