// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::engine::{SearchCategory, SearchHit};
use std::collections::HashMap;

const CN_STOPWORDS: &[&str] = &[
    "de", "le", "he", "shi", "zai", "wo", "you", "ta",
    "keyi", "ruhe", "shenme", "zenme",
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

const MAX_KEYWORDS: usize = 24;

const CN_CHAR_STOPWORDS: &[char] = &[
    '的', '了', '是', '在', '我', '你', '他', '她', '它', '和', '与', '或', '及',
    '也', '都', '就', '而', '被', '把', '让', '给', '为', '对', '从', '到', '很',
];

fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{20000}'..='\u{2A6DF}'
        | '\u{2A700}'..='\u{2B73F}'
        | '\u{F900}'..='\u{FAFF}'
    )
}

fn push_keyword(keyword: String, out: &mut Vec<String>, seen: &mut std::collections::HashSet<String>) {
    if out.len() >= MAX_KEYWORDS {
        return;
    }
    if seen.insert(keyword.clone()) {
        out.push(keyword);
    }
}

fn push_token_keywords(
    token: &str,
    stopset: &std::collections::HashSet<&str>,
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    let mut ascii_run = String::new();
    let mut cjk_run: Vec<char> = Vec::new();
    let flush_ascii = |run: &mut String, out: &mut Vec<String>, seen: &mut std::collections::HashSet<String>| {
        if run.chars().count() >= 2 && !stopset.contains(run.as_str()) {
            push_keyword(run.clone(), out, seen);
        }
        run.clear();
    };
    let flush_cjk = |run: &mut Vec<char>, out: &mut Vec<String>, seen: &mut std::collections::HashSet<String>| {
        match run.len() {
            0 => {}
            1 => {
                if !CN_CHAR_STOPWORDS.contains(&run[0]) {
                    push_keyword(run[0].to_string(), out, seen);
                }
            }
            _ => {
                for pair in run.windows(2) {
                    push_keyword(pair.iter().collect(), out, seen);
                }
            }
        }
        run.clear();
    };
    for c in token.chars() {
        if is_cjk_char(c) {
            flush_ascii(&mut ascii_run, out, seen);
            cjk_run.push(c);
        } else {
            flush_cjk(&mut cjk_run, out, seen);
            ascii_run.push(c);
        }
    }
    flush_ascii(&mut ascii_run, out, seen);
    flush_cjk(&mut cjk_run, out, seen);
}

pub fn extract_keywords(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
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
        if out.len() >= MAX_KEYWORDS {
            break;
        }
        let trimmed = token.trim();
        if trimmed.is_empty() || stopset.contains(trimmed) {
            continue;
        }
        if trimmed.chars().any(is_cjk_char) {
            push_token_keywords(trimmed, &stopset, &mut out, &mut seen);
        } else {
            if trimmed.chars().count() < 2 {
                continue;
            }
            push_keyword(trimmed.to_string(), &mut out, &mut seen);
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
    let matched: Vec<SearchHit> = hits
        .iter()
        .filter(|h| {
            let blob = format!("{} {}", h.title, h.description).to_lowercase();
            keywords.iter().any(|kw| blob.contains(kw.as_str()))
        })
        .cloned()
        .collect();
    if matched.is_empty() { hits } else { matched }
}

fn engine_tier(engine_id: &str, category: SearchCategory) -> f32 {
    let id = engine_id.trim().to_ascii_lowercase();
    let general = matches!(
        id.as_str(),
        "bing" | "duckduckgo" | "brave" | "serper" | "tavily" | "exa" | "google_news"
    );
    if general {
        return 1.0;
    }
    let category_match = match category {
        SearchCategory::Code => matches!(
            id.as_str(),
            "github"
                | "github_code_search"
                | "github_issues"
                | "github_advanced"
                | "gitlab"
                | "gitee"
                | "stackoverflow"
        ),
        SearchCategory::Academic => matches!(
            id.as_str(),
            "arxiv"
                | "openalex"
                | "semantic_scholar"
                | "crossref"
                | "dblp"
                | "pubmed"
                | "google_scholar"
                | "hal"
                | "core"
                | "biorxiv"
                | "ssrn"
                | "ieee_xplore"
        ),
        SearchCategory::News => matches!(
            id.as_str(),
            "google_news" | "bing_news" | "yahoo_news" | "thepaper" | "sohu" | "ithome" | "kr36"
        ),
        SearchCategory::Cn => matches!(
            id.as_str(),
            "baidu" | "csdn" | "juejin" | "weixin" | "zhihu" | "bilibili" | "weibo"
        ),
        SearchCategory::Social => matches!(
            id.as_str(),
            "reddit" | "zhihu" | "weibo" | "mastodon" | "v2ex" | "hackernews"
        ),
        SearchCategory::Forum => matches!(
            id.as_str(),
            "stackoverflow" | "hackernews" | "dev_to" | "v2ex" | "segmentfault" | "reddit"
        ),
        SearchCategory::Video => matches!(id.as_str(), "bilibili" | "invidious"),
        SearchCategory::Wiki => matches!(id.as_str(), "wikipedia"),
        _ => false,
    };
    if category_match {
        return 1.0;
    }
    match id.as_str() {
        "baidu" | "jina" | "searxng" | "wikipedia" | "google_scholar" | "stackoverflow" => 0.9,
        "csdn" | "juejin" | "weixin" | "zhihu" | "hackernews" | "reddit" | "github" => 0.8,
        _ => 0.6,
    }
}

fn freshness_bonus(published_at: &str) -> f32 {
    let date_prefix: String = published_at.chars().take(10).collect();
    let parsed = chrono::DateTime::parse_from_rfc3339(published_at)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(&date_prefix, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|dt| dt.and_utc())
        });
    let Some(ts) = parsed else {
        return 0.0;
    };
    let age_days = (chrono::Utc::now() - ts).num_days();
    if age_days < 0 {
        return 0.1;
    }
    match age_days {
        0..=1 => 0.2,
        2..=7 => 0.12,
        8..=30 => 0.05,
        _ => 0.0,
    }
}

pub fn score_and_rank(
    hits: Vec<SearchHit>,
    query: &str,
    category: SearchCategory,
    freshness_sensitive: bool,
) -> Vec<SearchHit> {
    if hits.len() <= 1 {
        return hits;
    }
    let keywords = extract_keywords(query);
    let phrase = query.trim().to_lowercase();
    let weigh_freshness = freshness_sensitive || matches!(category, SearchCategory::News);
    let mut scored: Vec<(f32, usize, SearchHit)> = hits
        .into_iter()
        .enumerate()
        .map(|(pos, hit)| {
            let blob_title = hit.title.to_lowercase();
            let blob_desc = hit.description.to_lowercase();
            let coverage = if keywords.is_empty() {
                0.5
            } else {
                let title_hits = keywords
                    .iter()
                    .filter(|kw| blob_title.contains(kw.as_str()))
                    .count() as f32;
                let desc_hits = keywords
                    .iter()
                    .filter(|kw| blob_desc.contains(kw.as_str()))
                    .count() as f32;
                (3.0 * title_hits + desc_hits) / (4.0 * keywords.len() as f32)
            };
            let all_words = !keywords.is_empty()
                && keywords
                    .iter()
                    .all(|kw| blob_title.contains(kw.as_str()) || blob_desc.contains(kw.as_str()));
            let phrase_min = if phrase.chars().any(is_cjk_char) { 3 } else { 6 };
            let phrase_hit = phrase.chars().count() >= phrase_min
                && (blob_title.contains(&phrase) || blob_desc.contains(&phrase));
            let engines: Vec<&str> = hit.engine.split('+').collect();
            let tier = engines
                .iter()
                .map(|id| engine_tier(id, category))
                .fold(0.0_f32, f32::max);
            let corroboration = 0.1 * ((engines.len().saturating_sub(1)).min(3) as f32);
            let native_score = hit
                .score
                .map(|s| 0.05 * s.clamp(0.0, 1.0))
                .unwrap_or(0.0);
            let freshness = if weigh_freshness {
                hit.published_at
                    .as_deref()
                    .map(freshness_bonus)
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            let position_prior = 0.15 / (2.0 + pos as f32).log2();
            let mut score = coverage
                + 0.35 * tier
                + corroboration
                + native_score
                + freshness
                + position_prior;
            if all_words {
                score += 0.15;
            }
            if phrase_hit {
                score += 0.2;
            }
            (score, pos, hit)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    scored.into_iter().map(|(_, _, hit)| hit).collect()
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
