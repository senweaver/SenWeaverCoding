// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::schema::QueryClassificationConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationDecision {
    pub hint: String,
    pub priority: i32,
}

pub fn classify(config: &QueryClassificationConfig, message: &str) -> Option<String> {
    classify_with_decision(config, message).map(|decision| decision.hint)
}

pub fn classify_with_decision(
    config: &QueryClassificationConfig,
    message: &str,
) -> Option<ClassificationDecision> {
    if !config.enabled || config.rules.is_empty() {
        return None;
    }

    let lower = message.to_lowercase();
    let len = message.len();

    let mut rules: Vec<_> = config.rules.iter().collect();
    rules.sort_by(|a, b| b.priority.cmp(&a.priority));

    for rule in rules {

        if let Some(min) = rule.min_length {
            if len < min {
                continue;
            }
        }
        if let Some(max) = rule.max_length {
            if len > max {
                continue;
            }
        }

        let keyword_hit = rule
            .keywords
            .iter()
            .any(|kw: &String| lower.contains(&kw.to_lowercase()));
        let pattern_hit = rule
            .patterns
            .iter()
            .any(|pat: &String| message.contains(pat.as_str()));

        if keyword_hit || pattern_hit {
            return Some(ClassificationDecision {
                hint: rule.hint.clone(),
                priority: rule.priority,
            });
        }
    }

    None
}
