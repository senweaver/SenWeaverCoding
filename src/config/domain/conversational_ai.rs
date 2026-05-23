// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConversationalAiConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_language")]
    pub default_language: String,

    #[serde(default = "default_supported_languages")]
    pub supported_languages: Vec<String>,

    #[serde(default = "default_true_bool")]
    pub auto_detect_language: bool,

    #[serde(default = "default_escalation_threshold")]
    pub escalation_confidence_threshold: f64,

    #[serde(default = "default_max_turns")]
    pub max_conversation_turns: usize,

    #[serde(default = "default_timeout_secs")]
    pub conversation_timeout_secs: u64,

    #[serde(default)]
    pub analytics_enabled: bool,

    #[serde(default)]
    pub knowledge_base_tool: Option<String>,
}

impl ConversationalAiConfig {

    pub fn is_disabled(&self) -> bool {
        !self.enabled
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if !self.enabled {
            return errors;
        }
        if self.default_language.trim().is_empty() {
            errors.push("conversational_ai.default_language must be non-empty".into());
        }
        if self.supported_languages.is_empty() {
            errors.push(
                "conversational_ai.supported_languages must list at least one language".into(),
            );
        }
        if !self.supported_languages.contains(&self.default_language) {
            errors.push(format!(
                "conversational_ai.default_language '{}' is not in supported_languages {:?}",
                self.default_language, self.supported_languages
            ));
        }
        if !(0.0..=1.0).contains(&self.escalation_confidence_threshold) {
            errors.push(format!(
                "conversational_ai.escalation_confidence_threshold {} must be in [0.0, 1.0]",
                self.escalation_confidence_threshold
            ));
        }
        if self.max_conversation_turns == 0 {
            errors.push("conversational_ai.max_conversation_turns must be >= 1".into());
        }
        if self.conversation_timeout_secs == 0 {
            errors.push("conversational_ai.conversation_timeout_secs must be >= 1".into());
        }
        errors
    }
}

impl Default for ConversationalAiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_language: default_language(),
            supported_languages: default_supported_languages(),
            auto_detect_language: true,
            escalation_confidence_threshold: default_escalation_threshold(),
            max_conversation_turns: default_max_turns(),
            conversation_timeout_secs: default_timeout_secs(),
            analytics_enabled: false,
            knowledge_base_tool: None,
        }
    }
}

pub(crate) fn default_language() -> String {
    "en".into()
}
pub(crate) fn default_supported_languages() -> Vec<String> {
    vec!["en".into(), "de".into(), "fr".into(), "it".into()]
}
pub(crate) fn default_escalation_threshold() -> f64 {
    0.3
}
pub(crate) fn default_max_turns() -> usize {
    50
}
pub(crate) fn default_timeout_secs() -> u64 {
    1800
}
pub(crate) fn default_true_bool() -> bool {
    true
}
