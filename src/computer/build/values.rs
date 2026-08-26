// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

static TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\{\{\s*([a-z0-9_]+)\s*\}\}").expect("token regex"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedValue {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
}

pub fn slugify_value_id(raw: &str) -> String {
    let mut slug = String::new();
    let mut prev_us = false;
    for c in raw.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            prev_us = false;
        } else if !prev_us && !slug.is_empty() {
            slug.push('_');
            prev_us = true;
        }
        if slug.chars().count() >= 40 {
            break;
        }
    }
    let slug = slug.trim_matches('_').to_string();
    if slug.is_empty() {
        "value".to_string()
    } else {
        slug
    }
}

pub fn parse_values(raw: Option<&serde_json::Value>) -> Vec<FixedValue> {
    let Some(array) = raw.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(|v| v.as_str())?;
            let id = slugify_value_id(id);
            Some(FixedValue {
                id,
                name: entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                value: entry
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

pub fn render_values(text: &str, values: &[FixedValue]) -> String {
    TOKEN_RE
        .replace_all(text, |caps: &regex::Captures| {
            let id = &caps[1];
            values
                .iter()
                .find(|v| v.id == id)
                .map(|v| v.value.clone())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

pub fn unresolved_tokens(text: &str, values: &[FixedValue]) -> Vec<String> {
    let mut out = Vec::new();
    for caps in TOKEN_RE.captures_iter(text) {
        let id = caps[1].to_string();
        if !values.iter().any(|v| v.id == id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}
