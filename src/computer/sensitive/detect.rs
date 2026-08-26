// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SensitiveCategory {
    PrivateKey,
    ApiKey,
    Jwt,
    Password,
    Email,
    CreditCard,
    Ssn,
    Phone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensitiveSeverity {
    High,
    Medium,
    Low,
}

impl SensitiveSeverity {
    fn rank(self) -> u8 {
        match self {
            SensitiveSeverity::High => 3,
            SensitiveSeverity::Medium => 2,
            SensitiveSeverity::Low => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SensitiveMatch {
    pub category: SensitiveCategory,
    pub label: &'static str,
    pub severity: SensitiveSeverity,
    pub value: String,
    pub start: usize,
    pub end: usize,
    pub rank: u32,
}

struct PiiSpec {
    category: SensitiveCategory,
    label: &'static str,
    severity: SensitiveSeverity,
    rank: u32,
    pattern: &'static Lazy<Regex>,
    digit_boundaries: bool,
    accept: Option<fn(&str) -> bool>,
}

static CARD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\d(?:[ -]?\d){12,18}").expect("card regex")
});
static SSN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\d{3}-\d{2}-\d{4}").expect("ssn regex")
});
static PHONE_NA_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:\+?1[\s.\-]?)?(?:\(\d{3}\)[\s.\-]?|\d{3}[\s.\-])\d{3}[\s.\-]\d{4}")
        .expect("phone regex")
});
static PHONE_E164_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\+\d(?:[\s.\-]?\d){7,14}").expect("e164 regex")
});
static EMAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").expect("email regex")
});

pub fn luhn_valid(digits: &str) -> bool {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for b in digits.bytes().rev() {
        let mut d = u32::from(b - b'0');
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum % 10 == 0
}

fn accept_card(value: &str) -> bool {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    (13..=19).contains(&digits.len()) && luhn_valid(&digits)
}

fn accept_ssn(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let (area, group, serial) = (parts[0], parts[1], parts[2]);
    area != "000" && area != "666" && !area.starts_with('9') && group != "00" && serial != "0000"
}

fn accept_phone_na(value: &str) -> bool {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    digits.len() == 10 || (digits.len() == 11 && digits.starts_with('1'))
}

fn accept_phone_e164(value: &str) -> bool {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    (8..=15).contains(&digits.len())
}

static PII_SPECS: [PiiSpec; 5] = [
    PiiSpec {
        category: SensitiveCategory::CreditCard,
        label: "Payment card number",
        severity: SensitiveSeverity::High,
        rank: 55,
        pattern: &CARD_RE,
        digit_boundaries: true,
        accept: Some(accept_card),
    },
    PiiSpec {
        category: SensitiveCategory::Ssn,
        label: "US Social Security number",
        severity: SensitiveSeverity::High,
        rank: 55,
        pattern: &SSN_RE,
        digit_boundaries: true,
        accept: Some(accept_ssn),
    },
    PiiSpec {
        category: SensitiveCategory::Phone,
        label: "Phone number",
        severity: SensitiveSeverity::Low,
        rank: 45,
        pattern: &PHONE_NA_RE,
        digit_boundaries: true,
        accept: Some(accept_phone_na),
    },
    PiiSpec {
        category: SensitiveCategory::Phone,
        label: "Phone number",
        severity: SensitiveSeverity::Low,
        rank: 45,
        pattern: &PHONE_E164_RE,
        digit_boundaries: true,
        accept: Some(accept_phone_e164),
    },
    PiiSpec {
        category: SensitiveCategory::Email,
        label: "Email address",
        severity: SensitiveSeverity::Medium,
        rank: 40,
        pattern: &EMAIL_RE,
        digit_boundaries: false,
        accept: None,
    },
];

fn digit_boundaries_ok(text: &str, start: usize, end: usize) -> bool {
    let before_ok = start == 0
        || !text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_digit());
    let after_ok = end >= text.len()
        || !text[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit());
    before_ok && after_ok
}

pub fn scan_structured_pii(text: &str) -> Vec<SensitiveMatch> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for spec in &PII_SPECS {
        for m in spec.pattern.find_iter(text) {
            let value = m.as_str();
            if spec.digit_boundaries && !digit_boundaries_ok(text, m.start(), m.end()) {
                continue;
            }
            if let Some(accept) = spec.accept {
                if !accept(value) {
                    continue;
                }
            }
            out.push(SensitiveMatch {
                category: spec.category,
                label: spec.label,
                severity: spec.severity,
                value: value.to_string(),
                start: m.start(),
                end: m.end(),
                rank: spec.rank,
            });
        }
    }
    resolve_overlaps(out)
}

pub fn resolve_overlaps(matches: Vec<SensitiveMatch>) -> Vec<SensitiveMatch> {
    let mut ordered = matches;
    ordered.sort_by(|a, b| {
        b.severity
            .rank()
            .cmp(&a.severity.rank())
            .then_with(|| b.rank.cmp(&a.rank))
            .then_with(|| (b.end - b.start).cmp(&(a.end - a.start)))
            .then_with(|| a.start.cmp(&b.start))
    });
    let mut kept: Vec<SensitiveMatch> = Vec::new();
    for m in ordered {
        if kept.iter().any(|k| m.start < k.end && k.start < m.end) {
            continue;
        }
        kept.push(m);
    }
    kept.sort_by_key(|m| m.start);
    kept
}

pub fn mask_value(value: &str) -> String {
    const MASK: &str = "••••";
    if value.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 6 {
        return MASK.to_string();
    }
    let head: String = chars[..2].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}{MASK}{tail}")
}

pub fn redact_text(text: &str, matches: &[SensitiveMatch]) -> String {
    if matches.is_empty() {
        return text.to_string();
    }
    let mut ordered: Vec<&SensitiveMatch> = matches.iter().collect();
    ordered.sort_by_key(|m| m.start);
    let mut out = String::new();
    let mut cursor = 0usize;
    for m in ordered {
        if m.start < cursor || m.end > text.len() {
            continue;
        }
        out.push_str(&text[cursor..m.start]);
        out.push_str(&mask_value(&m.value));
        cursor = m.end;
    }
    out.push_str(&text[cursor..]);
    out
}

pub fn redacted_snippet(text: &str, focus: &SensitiveMatch, all: &[SensitiveMatch]) -> String {
    const PAD: usize = 32;
    let from = focus.start.saturating_sub(PAD);
    let to = (focus.end + PAD).min(text.len());
    let from = floor_char_boundary(text, from);
    let to = ceil_char_boundary(text, to);
    let slice = &text[from..to];
    let local: Vec<SensitiveMatch> = all
        .iter()
        .filter(|m| m.end > from && m.start < to)
        .map(|m| SensitiveMatch {
            start: m.start.saturating_sub(from),
            end: (m.end - from).min(slice.len()),
            ..m.clone()
        })
        .collect();
    let redacted = redact_text(slice, &local)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{}{redacted}{}",
        if from > 0 { "…" } else { "" },
        if to < text.len() { "…" } else { "" }
    )
}

fn floor_char_boundary(text: &str, mut idx: usize) -> usize {
    idx = idx.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn ceil_char_boundary(text: &str, mut idx: usize) -> usize {
    idx = idx.min(text.len());
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

pub fn shannon_entropy(text: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    let mut counts: std::collections::HashMap<char, usize> = std::collections::HashMap::new();
    let mut total = 0usize;
    for c in text.chars() {
        *counts.entry(c).or_insert(0) += 1;
        total += 1;
    }
    let total_f = total as f64;
    counts
        .values()
        .map(|&count| {
            let p = count as f64 / total_f;
            -p * p.log2()
        })
        .sum()
}
