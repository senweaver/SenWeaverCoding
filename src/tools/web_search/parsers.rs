// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use regex::Regex;
use std::sync::LazyLock;

static STRIP_TAGS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[^>]+>").expect("web_search strip_tags regex"));

static HTML_ENTITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&(amp|lt|gt|quot|apos|nbsp|#x[0-9a-fA-F]+|#\d+);").expect("html entity regex"));

pub fn strip_tags(content: &str) -> String {
    STRIP_TAGS_RE.replace_all(content, "").to_string()
}

pub fn decode_html_entities(input: &str) -> String {
    HTML_ENTITY_RE
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let entity = &caps[1];
            match entity {
                "amp" => "&".to_string(),
                "lt" => "<".to_string(),
                "gt" => ">".to_string(),
                "quot" => "\"".to_string(),
                "apos" => "'".to_string(),
                "nbsp" => " ".to_string(),
                e if e.starts_with("#x") || e.starts_with("#X") => {
                    let hex = &e[2..];
                    u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                        .unwrap_or_default()
                }
                e if e.starts_with('#') => {
                    let dec = &e[1..];
                    dec.parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                        .unwrap_or_default()
                }
                _ => caps[0].to_string(),
            }
        })
        .to_string()
}

pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn clean_text(s: &str) -> String {
    let stripped = strip_tags(s);
    let decoded = decode_html_entities(&stripped);
    collapse_whitespace(&decoded).trim().to_string()
}

pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}

pub fn decode_ddg_redirect_url(raw_url: &str) -> String {
    if let Some(index) = raw_url.find("uddg=") {
        let encoded = &raw_url[index + 5..];
        let encoded = encoded.split('&').next().unwrap_or(encoded);
        if let Ok(decoded) = urlencoding::decode(encoded) {
            return decoded.into_owned();
        }
    }
    raw_url.to_string()
}
