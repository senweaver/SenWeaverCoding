// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use once_cell::sync::Lazy;
use regex::Regex;

static NO_MODEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)no[_\s\-]?model[_\s\-]?configured|please\s+add\s+at\s+least\s+one\s+model|未添加模型")
        .expect("no-model-configured error regex must compile")
});

pub fn is_no_model_error(message: &str) -> bool {
    NO_MODEL_RE.is_match(message)
}

pub fn classify_turn_error_code(message: &str) -> &'static str {
    if is_no_model_error(message) {
        "NO_MODEL_CONFIGURED"
    } else {
        "AGENT_TURN_FAILED"
    }
}
