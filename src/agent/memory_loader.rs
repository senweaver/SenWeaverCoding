// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::memory::{self, Memory, decay};
use async_trait::async_trait;
use std::fmt::Write;

#[async_trait]
pub trait MemoryLoader: Send + Sync {
    async fn load_context(
        &self,
        memory: &dyn Memory,
        user_message: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<String>;
}

pub struct DefaultMemoryLoader {
    limit: usize,
    min_relevance_score: f64,
}

impl Default for DefaultMemoryLoader {
    fn default() -> Self {
        Self {
            limit: 5,
            min_relevance_score: 0.4,
        }
    }
}

impl DefaultMemoryLoader {
    pub fn new(limit: usize, min_relevance_score: f64) -> Self {
        Self {
            limit: limit.max(1),
            min_relevance_score,
        }
    }
}

#[async_trait]
impl MemoryLoader for DefaultMemoryLoader {
    async fn load_context(
        &self,
        memory: &dyn Memory,
        user_message: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut entries = memory
            .recall(user_message, self.limit, session_id, None, None)
            .await?;
        if entries.is_empty() {
            return Ok(String::new());
        }

        decay::apply_time_decay(&mut entries, decay::DEFAULT_HALF_LIFE_DAYS);

        let mut context = String::from("[Memory context]\n");
        for entry in entries {
            if memory::is_assistant_autosave_key(&entry.key) {
                continue;
            }
            if memory::should_skip_autosave_content(&entry.content) {
                continue;
            }
            if let Some(score) = entry.score {
                if score < self.min_relevance_score {
                    continue;
                }
            }
            let _ = writeln!(context, "- {}: {}", entry.key, entry.content);
        }

        if context == "[Memory context]\n" {
            return Ok(String::new());
        }

        context.push_str("[/Memory context]\n\n");
        Ok(context)
    }
}
