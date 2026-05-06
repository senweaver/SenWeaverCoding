// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Event pattern matching for the routines engine.
//!
//! Supports three match strategies: exact, glob, and regex.  Each routine
//! declares one or more [`EventPattern`]s; an incoming [`RoutineEvent`] fires
//! the routine when **any** pattern matches.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchStrategy {

    #[default]
    Exact,

    Glob,

    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventPattern {

    pub source: String,

    pub pattern: String,

    #[serde(default)]
    pub strategy: MatchStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineEvent {

    pub source: String,

    pub topic: String,

    #[serde(default)]
    pub payload: Option<String>,

    pub timestamp: String,
}

pub fn matches(pattern: &EventPattern, event: &RoutineEvent) -> bool {
    if pattern.source != event.source {
        return false;
    }
    match pattern.strategy {
        MatchStrategy::Exact => pattern.pattern == event.topic,
        MatchStrategy::Glob => glob_match(&pattern.pattern, &event.topic),
        MatchStrategy::Regex => regex_match(&pattern.pattern, &event.topic),
    }
}

pub fn matches_any(patterns: &[EventPattern], event: &RoutineEvent) -> bool {
    patterns.iter().any(|p| matches(p, event))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    glob::Pattern::new(pattern).map_or(false, |g| g.matches(text))
}

fn regex_match(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).map_or(false, |re| re.is_match(text))
}
