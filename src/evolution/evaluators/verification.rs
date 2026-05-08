// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use chrono::Utc;

use super::FastEvaluator;
use crate::evolution::types::{SignalScore, SignalSource, ToolOutcome, TurnRecord};

const VERIFICATION_TOOL_PREFIXES: &[&str] = &[
    "shell",
    "bash",
    "exec",
    "run_command",
    "verify",
    "test",
    "lint",
];

const VERIFICATION_PAYLOAD_HINTS: &[&str] = &[
    "cargo check",
    "cargo build",
    "cargo clippy",
    "cargo test",
    "npm test",
    "npm run test",
    "pnpm test",
    "yarn test",
    "pytest",
    "go test",
    "go vet",
    "go build",
    "mvn test",
    "gradle test",
    "ctest",
    "tsc",
    "eslint",
    "prettier --check",
    "ruff",
    "mypy",
    "flake8",
    "black --check",
    "phpunit",
    "rspec",
    "ginkgo",
];

pub struct VerificationEvaluator;

impl FastEvaluator for VerificationEvaluator {
    fn evaluate(&self, turn: &TurnRecord) -> Option<SignalScore> {
        if turn.tool_outcomes.is_empty() {
            return None;
        }
        let mut hits: Vec<&ToolOutcome> = Vec::new();
        for outcome in &turn.tool_outcomes {
            if is_verification_tool(outcome) {
                hits.push(outcome);
            }
        }
        if hits.is_empty() {
            return None;
        }
        let total = hits.len() as f32;
        let ok = hits.iter().filter(|o| o.ok).count() as f32;
        let score = (ok / total * 2.0 - 1.0).clamp(-1.0, 1.0);
        Some(SignalScore {
            source: SignalSource::Verification,
            score,
            confidence: total.min(4.0) / 4.0,
            reason: Some(format!(
                "{}/{} verification-class tools succeeded",
                ok as u64, total as u64
            )),
            ts: Utc::now(),
        })
    }
}

fn is_verification_tool(outcome: &ToolOutcome) -> bool {
    let lower_name = outcome.name.to_lowercase();
    let name_signals_verification = lower_name.contains("test")
        || lower_name.contains("verify")
        || lower_name.contains("lint")
        || lower_name.contains("typecheck")
        || lower_name.contains("check");
    let name_is_shell = VERIFICATION_TOOL_PREFIXES
        .iter()
        .any(|prefix| lower_name == *prefix || lower_name.starts_with(prefix));
    if name_signals_verification {
        return true;
    }
    if name_is_shell && matches_command_hint(outcome) {
        return true;
    }
    false
}

fn matches_command_hint(outcome: &ToolOutcome) -> bool {
    let mut haystack = String::new();
    if let Some(payload) = &outcome.payload_excerpt {
        haystack.push_str(&payload.to_lowercase());
    }
    if let Some(args) = &outcome.arguments {
        haystack.push('\n');
        haystack.push_str(&args.to_string().to_lowercase());
    }
    if let Some(err) = &outcome.error_excerpt {
        haystack.push('\n');
        haystack.push_str(&err.to_lowercase());
    }
    if haystack.is_empty() {
        return false;
    }
    VERIFICATION_PAYLOAD_HINTS
        .iter()
        .any(|hint| haystack.contains(hint))
}
