// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::engine::SearchHit;
use std::collections::HashMap;

const CN_STOPWORDS: &[&str] = &[
    "的", "是", "在", "了", "和", "与", "或", "则", "而", "但", "可以", "如何", "什么", "怎么",
    "这", "那", "一个", "一些", "有", "要", "从", "用", "为", "以", "到", "就", "上", "下",
    "我们", "他们", "你们",
];
const EN_STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could", "should",
    "may", "might", "must", "shall", "can", "need", "dare", "ought", "used",
    "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into",
    "through", "during", "before", "after", "above", "below", "between",
    "and", "or", "but", "if", "because", "while", "although", "though", "that",
    "this", "these", "those", "it", "its",
];

pub fn extract_keywords(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() || c >= '\u{4e00}' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let stopset: std::collections::HashSet<&str> = CN_STOPWORDS
        .iter()
        .chain(EN_STOPWORDS.iter())
        .copied()
        .collect();
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for token in cleaned.split_whitespace() {
        let trimmed = token.trim();
        if trimmed.chars().count() < 2 {
            continue;
        }
        if stopset.contains(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

pub fn round_robin_interleave(streams: Vec<Vec<SearchHit>>) -> Vec<SearchHit> {
    if streams.is_empty() {
        return Vec::new();
    }
    let max_len = streams.iter().map(|s| s.len()).max().unwrap_or(0);
    let total: usize = streams.iter().map(|s| s.len()).sum();
    let mut out = Vec::with_capacity(total);
    for col in 0..max_len {
        for stream in streams.iter() {
            if let Some(hit) = stream.get(col) {
                out.push(hit.clone());
            }
        }
    }
    out
}

pub fn merge_and_dedup(streams: Vec<Vec<SearchHit>>, max_results: usize) -> Vec<SearchHit> {
    if streams.is_empty() {
        return Vec::new();
    }
    let interleaved = round_robin_interleave(streams);
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut merged: Vec<SearchHit> = Vec::new();
    for hit in interleaved {
        let key = hit.dedup_key();
        if key.is_empty() {
            merged.push(hit);
            continue;
        }
        if let Some(&idx) = seen.get(&key) {
            let existing = &mut merged[idx];
            if hit.description.len() > existing.description.len() {
                existing.description = hit.description.clone();
            }
            if existing.source.is_none() && hit.source.is_some() {
                existing.source = hit.source.clone();
            }
            if existing.published_at.is_none() && hit.published_at.is_some() {
                existing.published_at = hit.published_at.clone();
            }
            let already = existing
                .engine
                .split('+')
                .any(|e| e == hit.engine);
            if !already {
                existing.engine = format!("{}+{}", existing.engine, hit.engine);
            }
        } else {
            seen.insert(key, merged.len());
            merged.push(hit);
        }
        if merged.len() >= max_results.saturating_mul(3) {
            break;
        }
    }
    merged.truncate(max_results);
    merged
}

pub fn academic_merge(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut merged: Vec<SearchHit> = Vec::new();
    for hit in hits {
        let key = hit.academic_dedup_key();
        let Some(key) = key else {
            merged.push(hit);
            continue;
        };
        if let Some(&idx) = seen.get(&key) {
            let existing = &mut merged[idx];
            if hit.description.len() > existing.description.len() {
                existing.description = hit.description.clone();
            }
            if existing.source.is_none() && hit.source.is_some() {
                existing.source = hit.source.clone();
            }
            if existing.published_at.is_none() && hit.published_at.is_some() {
                existing.published_at = hit.published_at.clone();
            }
            let already = existing
                .engine
                .split('+')
                .any(|e| e == hit.engine);
            if !already {
                existing.engine = format!("{}+{}", existing.engine, hit.engine);
            }
            for (k, v) in hit.extra.iter() {
                existing.extra.entry(k.clone()).or_insert_with(|| v.clone());
            }
        } else {
            seen.insert(key, merged.len());
            merged.push(hit);
        }
    }
    merged
}

pub fn filter_by_relevance(hits: Vec<SearchHit>, query: &str) -> Vec<SearchHit> {
    let keywords = extract_keywords(query);
    if keywords.is_empty() {
        return hits;
    }
    hits.into_iter()
        .filter(|h| {
            let blob = format!("{} {}", h.title, h.description).to_lowercase();
            keywords.iter().any(|kw| blob.contains(kw.as_str()))
        })
        .collect()
}

pub fn render_results_markdown(query: &str, hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return format!("No results found for query: {query}");
    }
    let mut out = String::new();
    out.push_str(&format!("Search results for: {query}\n"));
    out.push_str(&format!("Found {} result(s):\n\n", hits.len()));
    for (idx, hit) in hits.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", idx + 1, hit.title));
        out.push_str(&format!("    URL: {}\n", hit.url));
        out.push_str(&format!("    Engine: {}\n", hit.engine));
        if let Some(source) = &hit.source {
            out.push_str(&format!("    Source: {source}\n"));
        }
        if let Some(pub_at) = &hit.published_at {
            out.push_str(&format!("    Published: {pub_at}\n"));
        }
        if !hit.description.is_empty() {
            out.push_str(&format!("    {}\n", hit.description));
        }
        out.push('\n');
    }
    out
}
