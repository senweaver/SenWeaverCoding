// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct QueryPlan {
    pub raw_query: String,
    pub tokens: Vec<String>,
    pub phrases: Vec<String>,
    pub languages: Vec<String>,
    pub intent: QueryIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    Concept,
    Implementation,
    Usage,
    Configuration,
    Documentation,
}

impl QueryPlan {
    pub fn relaxed_for(&self, missing: &[String]) -> QueryPlan {
        let mut tokens: Vec<String> = self
            .tokens
            .iter()
            .filter(|t| missing.iter().any(|m| m == *t))
            .cloned()
            .collect();
        for token in missing {
            let trimmed = token.trim();
            if trimmed.len() <= 4 {
                continue;
            }
            tokens.push(trimmed.chars().take(trimmed.len().saturating_sub(2)).collect());
        }
        QueryPlan {
            raw_query: self.raw_query.clone(),
            tokens,
            phrases: Vec::new(),
            languages: self.languages.clone(),
            intent: self.intent,
        }
    }
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "and", "or", "for", "is", "are", "was", "were", "be", "in", "on",
    "at", "by", "as", "with", "this", "that", "it", "its", "from", "have", "has", "had", "but",
    "not", "if", "then", "than", "do", "does", "did", "we", "you", "i", "they", "he", "she",
    "我们", "你们", "他们", "这个", "那个", "是", "的", "在", "和",
];

pub fn plan_query(raw_query: &str, languages: &[String]) -> QueryPlan {
    let lowered = raw_query.to_lowercase();
    let intent = classify_intent(&lowered);
    let phrases = extract_phrases(raw_query);
    let mut tokens: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for raw_token in tokenize_loose(raw_query) {
        let lc = raw_token.to_lowercase();
        if STOPWORDS.contains(&lc.as_str()) {
            continue;
        }
        if lc.chars().all(|c| c.is_ascii_digit()) && lc.len() < 3 {
            continue;
        }
        if seen.insert(lc.clone()) {
            tokens.push(lc);
        }
    }
    QueryPlan {
        raw_query: raw_query.to_string(),
        tokens,
        phrases,
        languages: languages.to_vec(),
        intent,
    }
}

fn extract_phrases(raw_query: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut buf = String::new();
    let mut inside = false;
    for ch in raw_query.chars() {
        if ch == '"' {
            if inside && !buf.trim().is_empty() {
                phrases.push(buf.trim().to_string());
                buf.clear();
            }
            inside = !inside;
            continue;
        }
        if inside {
            buf.push(ch);
        }
    }
    phrases
}

fn tokenize_loose(raw_query: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in raw_query.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            cur.push(ch);
        } else {
            if !cur.is_empty() {
                push_camel_split(&cur, &mut tokens);
                cur.clear();
            }
        }
    }
    if !cur.is_empty() {
        push_camel_split(&cur, &mut tokens);
    }
    tokens
}

fn push_camel_split(raw: &str, out: &mut Vec<String>) {
    out.push(raw.to_string());
    let has_upper = raw.chars().any(|c| c.is_uppercase());
    if !has_upper {
        return;
    }
    let mut buf = String::new();
    for ch in raw.chars() {
        if ch.is_uppercase() && !buf.is_empty() {
            out.push(buf.clone());
            buf.clear();
        }
        buf.push(ch);
    }
    if !buf.is_empty() {
        out.push(buf);
    }
}

fn classify_intent(lowered: &str) -> QueryIntent {
    if lowered.contains("config")
        || lowered.contains("setting")
        || lowered.contains("toml")
        || lowered.contains("yaml")
        || lowered.contains("env")
    {
        return QueryIntent::Configuration;
    }
    if lowered.contains("how to")
        || lowered.contains("how do")
        || lowered.contains("usage")
        || lowered.contains("example")
        || lowered.contains("call")
    {
        return QueryIntent::Usage;
    }
    if lowered.contains("implement")
        || lowered.contains("impl ")
        || lowered.contains("function")
        || lowered.contains("class")
        || lowered.contains("trait")
        || lowered.contains("struct")
    {
        return QueryIntent::Implementation;
    }
    if lowered.contains("doc")
        || lowered.contains("readme")
        || lowered.contains("changelog")
        || lowered.contains("guide")
    {
        return QueryIntent::Documentation;
    }
    QueryIntent::Concept
}
