// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::providers::traits::{ChatMessage, ChatRequest, Provider};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AutoTitleConfig {

    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default = "default_max_length")]
    pub max_length: usize,

    #[serde(default = "default_trigger_after")]
    pub trigger_after_exchanges: usize,
}

fn default_enabled() -> bool {
    true
}
fn default_max_length() -> usize {
    60
}
fn default_trigger_after() -> usize {
    1
}

impl Default for AutoTitleConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            max_length: default_max_length(),
            trigger_after_exchanges: default_trigger_after(),
        }
    }
}

const TITLE_PROMPT: &str = "Generate a concise title (max 60 chars) for this conversation. \
Return ONLY the title text, no quotes, no prefix, no explanation.";

pub async fn generate_title(
    provider: &dyn Provider,
    user_message: &str,
    assistant_response: &str,
    model: &str,
    config: &AutoTitleConfig,
) -> Option<String> {
    if !config.enabled {
        return None;
    }

    let context = if assistant_response.len() > 500 {
        format!(
            "User: {}\n\nAssistant: {}...",
            truncate(user_message, 300),
            truncate(assistant_response, 500),
        )
    } else {
        format!(
            "User: {}\n\nAssistant: {}",
            truncate(user_message, 300),
            assistant_response,
        )
    };

    let messages = vec![
        ChatMessage::system(TITLE_PROMPT),
        ChatMessage::user(&context),
    ];

    let request = ChatRequest {
        messages: &messages,
        tools: None,
    };

    match provider.chat(request, model, 0.3).await {
        Ok(response) => {
            let raw = response.text.as_deref().unwrap_or("").trim().to_string();
            let title = raw.trim_matches('"').trim_matches('\'').to_string();
            if title.is_empty() || title.len() > config.max_length * 2 {
                None
            } else {
                Some(truncate(&title, config.max_length).to_string())
            }
        }
        Err(e) => {
            tracing::debug!("Auto-title generation failed: {e}");
            None
        }
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
