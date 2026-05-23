// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub enum GuardResult {

    Safe,

    Suspicious(Vec<String>, f64),

    Blocked(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GuardAction {

    Warn,

    #[default]
    Block,

    Sanitize,
}

impl GuardAction {
    pub fn parse_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "block" => Self::Block,
            "sanitize" => Self::Sanitize,
            _ => Self::Warn,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptGuard {

    action: GuardAction,

    sensitivity: f64,
}

impl Default for PromptGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptGuard {

    pub fn new() -> Self {
        Self {
            action: GuardAction::Block,
            sensitivity: 0.7,
        }
    }

    pub fn with_config(action: GuardAction, sensitivity: f64) -> Self {
        Self {
            action,
            sensitivity: sensitivity.clamp(0.0, 1.0),
        }
    }

    pub fn scan(&self, content: &str) -> GuardResult {
        let mut detected_patterns = Vec::new();
        let mut total_score = 0.0;
        let mut max_score: f64 = 0.0;

        let score = self.check_system_override(content, &mut detected_patterns);
        total_score += score;
        max_score = max_score.max(score);

        let score = self.check_role_confusion(content, &mut detected_patterns);
        total_score += score;
        max_score = max_score.max(score);

        let score = self.check_tool_injection(content, &mut detected_patterns);
        total_score += score;
        max_score = max_score.max(score);

        let score = self.check_secret_extraction(content, &mut detected_patterns);
        total_score += score;
        max_score = max_score.max(score);

        let score = self.check_command_injection(content, &mut detected_patterns);
        total_score += score;
        max_score = max_score.max(score);

        let score = self.check_jailbreak_attempts(content, &mut detected_patterns);
        total_score += score;
        max_score = max_score.max(score);

        let num_categories = 6.0;
        let normalized_score = (total_score / num_categories).min(1.0);

        if detected_patterns.is_empty() {
            GuardResult::Safe
        } else {

            match self.action {
                GuardAction::Block if max_score > self.sensitivity => {
                    GuardResult::Blocked(format!(
                        "Potential prompt injection detected (max score: {:.2}): {}",
                        max_score,
                        detected_patterns.join(", ")
                    ))
                }
                _ => GuardResult::Suspicious(detected_patterns, normalized_score),
            }
        }
    }

    fn check_system_override(&self, content: &str, patterns: &mut Vec<String>) -> f64 {
        static SYSTEM_OVERRIDE_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
        let regexes = SYSTEM_OVERRIDE_PATTERNS.get_or_init(|| {
            vec![
                Regex::new(
                    r"(?i)ignore\s+((all\s+)?(previous|above|prior)|all)\s+(instructions?|prompts?|commands?)",
                )
                .expect("system override 'ignore' regex must compile"),
                Regex::new(r"(?i)disregard\s+(previous|all|above|prior)")
                    .expect("system override 'disregard' regex must compile"),
                Regex::new(r"(?i)forget\s+(previous|all|everything|above)")
                    .expect("system override 'forget' regex must compile"),
                Regex::new(r"(?i)new\s+(instructions?|rules?|system\s+prompt)")
                    .expect("system override 'new instructions' regex must compile"),
                Regex::new(r"(?i)override\s+(system|instructions?|rules?)")
                    .expect("system override 'override' regex must compile"),
                Regex::new(r"(?i)reset\s+(instructions?|context|system)")
                    .expect("system override 'reset' regex must compile"),
            ]
        });

        for regex in regexes {
            if regex.is_match(content) {
                patterns.push("system_prompt_override".to_string());
                return 1.0;
            }
        }
        0.0
    }

    fn check_role_confusion(&self, content: &str, patterns: &mut Vec<String>) -> f64 {
        static ROLE_CONFUSION_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
        let regexes = ROLE_CONFUSION_PATTERNS.get_or_init(|| {
            vec![
                Regex::new(
                    r"(?i)(you\s+are\s+now|act\s+as|pretend\s+(you're|to\s+be))\s+(a|an|the)?",
                )
                .expect("role confusion 'you are now' regex must compile"),
                Regex::new(r"(?i)(your\s+new\s+role|you\s+have\s+become|you\s+must\s+be)")
                    .expect("role confusion 'new role' regex must compile"),
                Regex::new(r"(?i)from\s+now\s+on\s+(you\s+are|act\s+as|pretend)")
                    .expect("role confusion 'from now on' regex must compile"),
                Regex::new(r"(?i)(assistant|AI|system|model):\s*\[?(system|override|new\s+role)")
                    .expect("role confusion 'speaker prefix' regex must compile"),
            ]
        });

        for regex in regexes {
            if regex.is_match(content) {
                patterns.push("role_confusion".to_string());
                return 0.9;
            }
        }
        0.0
    }

    fn check_tool_injection(&self, content: &str, patterns: &mut Vec<String>) -> f64 {

        if content.contains("tool_calls") || content.contains("function_call") {

            if content.contains(r#"{"type":"#) || content.contains(r#"{"name":"#) {
                patterns.push("tool_call_injection".to_string());
                return 0.8;
            }
        }

        if content.contains(r#"}"}"#) || content.contains(r"}'") {
            patterns.push("json_escape_attempt".to_string());
            return 0.7;
        }

        0.0
    }

    fn check_secret_extraction(&self, content: &str, patterns: &mut Vec<String>) -> f64 {
        static SECRET_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
        let regexes = SECRET_PATTERNS.get_or_init(|| {
            vec![
                Regex::new(r"(?i)(list|show|print|display|reveal|tell\s+me)\s+(all\s+)?(secrets?|credentials?|passwords?|tokens?|keys?)")
                    .expect("secret extraction 'list/show' regex must compile"),
                Regex::new(r"(?i)(what|show)\s+(are|is|me)\s+(all\s+)?(your|the)\s+(api\s+)?(keys?|secrets?|credentials?)")
                    .expect("secret extraction 'what/show' regex must compile"),
                Regex::new(r"(?i)contents?\s+of\s+(vault|secrets?|credentials?)")
                    .expect("secret extraction 'contents of' regex must compile"),
                Regex::new(r"(?i)(dump|export)\s+(vault|secrets?|credentials?)")
                    .expect("secret extraction 'dump/export' regex must compile"),
            ]
        });

        for regex in regexes {
            if regex.is_match(content) {
                patterns.push("secret_extraction".to_string());
                return 0.95;
            }
        }
        0.0
    }

    fn check_command_injection(&self, content: &str, patterns: &mut Vec<String>) -> f64 {

        let dangerous_patterns = [
            ("`", "backtick_execution"),
            ("$(", "command_substitution"),
            ("&&", "command_chaining"),
            ("||", "command_chaining"),
            (";", "command_separator"),
            ("|", "pipe_operator"),
            (">/dev/", "dev_redirect"),
            ("2>&1", "stderr_redirect"),
        ];

        let mut score = 0.0;
        for (pattern, name) in dangerous_patterns {
            if content.contains(pattern) {

                if pattern == "|"
                    && (content.contains("| head")
                        || content.contains("| tail")
                        || content.contains("| grep"))
                {
                    continue;
                }
                if pattern == "&&" && content.len() < 100 {

                    continue;
                }
                patterns.push(name.to_string());
                score = 0.6;
                break;
            }
        }
        score
    }

    fn check_jailbreak_attempts(&self, content: &str, patterns: &mut Vec<String>) -> f64 {
        static JAILBREAK_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
        let regexes = JAILBREAK_PATTERNS.get_or_init(|| {
            vec![

                Regex::new(r"(?i)\bDAN\b.*mode").expect("jailbreak 'DAN' regex must compile"),
                Regex::new(r"(?i)do\s+anything\s+now")
                    .expect("jailbreak 'do anything now' regex must compile"),

                Regex::new(r"(?i)enter\s+(developer|debug|admin)\s+mode")
                    .expect("jailbreak 'enter mode' regex must compile"),
                Regex::new(r"(?i)enable\s+(developer|debug|admin)\s+mode")
                    .expect("jailbreak 'enable mode' regex must compile"),

                Regex::new(r"(?i)in\s+this\s+hypothetical")
                    .expect("jailbreak 'hypothetical' regex must compile"),
                Regex::new(r"(?i)imagine\s+you\s+(have\s+no|don't\s+have)\s+(restrictions?|rules?|limits?)")
                    .expect("jailbreak 'imagine no restrictions' regex must compile"),

                Regex::new(r"(?i)decode\s+(this|the\s+following)\s+(base64|hex|rot13)")
                    .expect("jailbreak 'decode payload' regex must compile"),
            ]
        });

        for regex in regexes {
            if regex.is_match(content) {
                patterns.push("jailbreak_attempt".to_string());
                return 0.85;
            }
        }
        0.0
    }
}
