// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

static REGEX_CACHE: LazyLock<Mutex<HashMap<Arc<str>, Arc<regex::Regex>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
    let key: Arc<str> = Arc::from(pattern);
    let cached = {
        let guard = REGEX_CACHE.lock();
        guard.get(&key).cloned()
    };
    let re = match cached {
        Some(re) => re,
        None => match regex::Regex::new(pattern) {
            Ok(re) => {
                let arc = Arc::new(re);
                REGEX_CACHE.lock().insert(key, Arc::clone(&arc));
                arc
            }
            Err(_) => return false,
        },
    };
    re.is_match(text)
}
