// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use crate::hooks::traits::HookHandler;
use crate::tools::traits::ToolResult;

const MAX_RETAINED_ENTRIES: usize = 500;

pub struct CommandLoggerHook {
    log: Arc<Mutex<VecDeque<String>>>,
}

impl CommandLoggerHook {
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    #[must_use]
    pub fn recent(&self) -> Vec<String> {
        self.log.lock().iter().cloned().collect()
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
        let mut log = self.log.lock();
        log.push_back(entry);
        while log.len() > MAX_RETAINED_ENTRIES {
            log.pop_front();
        }
    }
}
