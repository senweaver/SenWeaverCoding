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

const TITLE_PROMPT: &str = "You write short titles for chat sessions. Given the user's first message and the assistant's reply, \
produce ONE concise title that names the concrete topic or task. Rules: write the title in the same language the user \
wrote in (Chinese request => Chinese title, English request => English title); keep it to at most 12 Chinese characters \
or 6 English words; do not use quotes, brackets, emoji, trailing punctuation, or any prefix such as 'Title:'. \
Return ONLY the title text.";

const PLACEHOLDER_TITLES: &[&str] = &[
    "untitled session",
    "untitled",
    "new session",
    "new conversation",
    "new agent",
    "新对话",
    "新智能体",
    "新建智能体",
    "未命名会话",
];

pub fn is_placeholder_title(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_lowercase();
    if PLACEHOLDER_TITLES.iter().any(|p| *p == lowered) {
        return true;
    }
    let bytes = trimmed.as_bytes();
    trimmed.len() == 13
        && trimmed.starts_with("Session ")
        && bytes[8].is_ascii_digit()
        && bytes[9].is_ascii_digit()
        && bytes[10] == b':'
        && bytes[11].is_ascii_digit()
        && bytes[12].is_ascii_digit()
}

pub fn provisional_title(content: &str, max_chars: usize) -> Option<String> {
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with("[IMAGE:")
            || line.starts_with("[Attached file:")
            || line.starts_with("[Attached image:")
        {
            continue;
        }
        let stripped = line
            .trim_start_matches(|c: char| matches!(c, '#' | '>' | '*' | '-' | '•') || c.is_whitespace())
            .trim();
        if stripped.is_empty() {
            continue;
        }
        let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
        let total = collapsed.chars().count();
        if total <= max_chars {
            return Some(collapsed);
        }
        let mut title: String = collapsed.chars().take(max_chars.max(1)).collect();
        title.push('…');
        return Some(title);
    }
    None
}

fn clean_generated_title(raw: &str) -> String {
    let mut title = raw.trim();
    for prefix in ["Title:", "title:", "标题：", "标题:"] {
        if let Some(rest) = title.strip_prefix(prefix) {
            title = rest.trim();
        }
    }
    let title = title
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    title
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '“' | '”' | '‘' | '’' | '《' | '》' | '「' | '」' | '`' | '*'))
        .trim_end_matches(|c: char| matches!(c, '.' | '。' | '!' | '！' | '?' | '？' | ',' | '，' | ';' | '；' | ':' | '：'))
        .trim()
        .to_string()
}

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
            let title = clean_generated_title(response.text.as_deref().unwrap_or(""));
            if title.is_empty()
                || title.len() > config.max_length * 2
                || is_placeholder_title(&title)
            {
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
