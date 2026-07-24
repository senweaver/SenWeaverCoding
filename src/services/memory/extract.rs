// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMemory {
    pub content: String,
    pub category: MemoryCategory,
    pub confidence: f64,
    pub source_turn: u32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Preference,
    Decision,
    Fact,
    Convention,
    ProjectStructure,
    Workflow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionConfig {
    pub enabled: bool,
    pub min_confidence: f64,
    pub max_per_turn: usize,
    pub categories: Vec<MemoryCategory>,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: 0.7,
            max_per_turn: 3,
            categories: vec![
                MemoryCategory::Preference,
                MemoryCategory::Decision,
                MemoryCategory::Fact,
                MemoryCategory::Convention,
            ],
        }
    }
}

fn find_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    let nlen = needle.len();
    if nlen == 0 || nlen > haystack.len() {
        return None;
    }
    haystack
        .as_bytes()
        .windows(nlen)
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
        .filter(|&i| haystack.is_char_boundary(i))
}

pub fn extract_from_turn(
    user_message: &str,
    assistant_response: &str,
    config: &ExtractionConfig,
) -> Vec<ExtractedMemory> {
    if !config.enabled {
        return Vec::new();
    }

    let mut memories = Vec::new();

    let preference_patterns = [
        "I prefer",
        "I like",
        "I always",
        "I never",
        "please always",
        "please never",
        "don't use",
        "always use",
        "my preferred",
    ];
    for pattern in &preference_patterns {
        if let Some(start) = find_ignore_ascii_case(user_message, pattern) {
            let end = user_message[start..]
                .find(['.', '!', '\n'])
                .map(|p| start + p + 1)
                .unwrap_or(user_message.len());
            let content = user_message[start..end].trim().to_string();
            if !content.is_empty() {
                memories.push(ExtractedMemory {
                    content,
                    category: MemoryCategory::Preference,
                    confidence: 0.85,
                    source_turn: 0,
                    tags: vec!["auto-extracted".to_string()],
                });
            }
        }
    }

    let convention_patterns = [
        "convention is",
        "standard is",
        "we use",
        "project uses",
        "codebase uses",
        "repo uses",
    ];
    for pattern in &convention_patterns {
        if let Some(pos) = find_ignore_ascii_case(assistant_response, pattern) {
            let start = assistant_response[..pos]
                .rfind(['.', '\n'])
                .map(|p| p + 1)
                .unwrap_or(0);
            let end = assistant_response[pos..]
                .find(['.', '\n'])
                .map(|p| pos + p + 1)
                .unwrap_or(assistant_response.len());
            let content = assistant_response[start..end].trim().to_string();
            if !content.is_empty() && content.len() < 500 {
                memories.push(ExtractedMemory {
                    content,
                    category: MemoryCategory::Convention,
                    confidence: 0.75,
                    source_turn: 0,
                    tags: vec!["auto-extracted".to_string()],
                });
            }
        }
    }

    memories.retain(|m| m.confidence >= config.min_confidence);
    memories.truncate(config.max_per_turn);
    memories
}
