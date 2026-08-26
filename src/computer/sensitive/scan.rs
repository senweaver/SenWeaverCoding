// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::detect::{
    mask_value, redacted_snippet, SensitiveCategory, SensitiveMatch, SensitiveSeverity,
};
use super::secrets::scan_text;

pub const SENSITIVE_REPORT_FILE: &str = "sensitive-report.json";
const MIN_REDACT_VALUE_LEN: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensitiveFinding {
    pub category: SensitiveCategory,
    pub label: String,
    pub severity: SensitiveSeverity,
    pub source: String,
    pub redacted_value: String,
    pub snippet: String,
    pub at_ms: Option<i64>,
    pub occurrences: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensitiveImagesSummary {
    pub frames_blurred: u32,
    pub regions_blurred: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensitiveReport {
    pub session_id: String,
    pub scanned_at: i64,
    pub total_findings: usize,
    pub high_severity_count: usize,
    pub counts: HashMap<String, usize>,
    pub findings: Vec<SensitiveFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<SensitiveImagesSummary>,
}

pub struct ScanOutcome {
    pub report: SensitiveReport,
    pub values: Vec<String>,
}

struct ScanTarget {
    text: String,
    source: &'static str,
    at_ms: Option<i64>,
}

fn category_key(category: SensitiveCategory) -> &'static str {
    match category {
        SensitiveCategory::PrivateKey => "private-key",
        SensitiveCategory::ApiKey => "api-key",
        SensitiveCategory::Jwt => "jwt",
        SensitiveCategory::Password => "password",
        SensitiveCategory::Email => "email",
        SensitiveCategory::CreditCard => "credit-card",
        SensitiveCategory::Ssn => "ssn",
        SensitiveCategory::Phone => "phone",
    }
}

fn payload_source(event_kind: &str, key: &str) -> &'static str {
    match (event_kind, key) {
        (_, "title") => "window-title",
        (_, "url") => "url",
        (_, "textPreview") => "clipboard",
        (_, "note") => "note",
        (kind, "value") if kind.starts_with("input.") => "input",
        _ => "other",
    }
}

fn collect_targets(dir: &Path) -> (Vec<ScanTarget>, i64) {
    let events = crate::computer::activity::events::read_events(dir);
    let started = events.first().map(|e| e.epoch).unwrap_or(0);
    let mut targets = Vec::new();

    for event in &events {
        let Some(map) = event.payload.as_object() else {
            continue;
        };
        for (key, value) in map {
            let Some(text) = value.as_str() else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            targets.push(ScanTarget {
                text: text.to_string(),
                source: payload_source(&event.kind, key),
                at_ms: Some(event.epoch - started),
            });
        }
    }

    if let Some(narration) = super::super::narration::load_transcript(dir) {
        for segment in narration.segments {
            if !segment.text.trim().is_empty() {
                targets.push(ScanTarget {
                    text: segment.text,
                    source: "narration",
                    at_ms: Some(segment.at_ms),
                });
            }
        }
    }

    (targets, started)
}

pub fn scan_recording(dir: &Path, session_id: &str) -> ScanOutcome {
    let (targets, _) = collect_targets(dir);

    let mut match_cache: HashMap<String, Vec<SensitiveMatch>> = HashMap::new();
    let mut findings: HashMap<(String, &'static str, String), SensitiveFinding> = HashMap::new();
    let mut values: Vec<String> = Vec::new();

    for target in &targets {
        let matches = match_cache
            .entry(target.text.clone())
            .or_insert_with(|| scan_text(&target.text));
        if matches.is_empty() {
            continue;
        }
        let all = matches.clone();
        for m in &all {
            if m.value.chars().count() >= MIN_REDACT_VALUE_LEN
                && !values.iter().any(|v| v == &m.value)
            {
                values.push(m.value.clone());
            }
            let key = (
                target.source.to_string(),
                category_key(m.category),
                m.value.clone(),
            );
            match findings.get_mut(&key) {
                Some(existing) => {
                    existing.occurrences += 1;
                    if let (Some(at), Some(prev)) = (target.at_ms, existing.at_ms) {
                        if at < prev {
                            existing.at_ms = Some(at);
                        }
                    }
                }
                None => {
                    findings.insert(
                        key,
                        SensitiveFinding {
                            category: m.category,
                            label: m.label.to_string(),
                            severity: m.severity,
                            source: target.source.to_string(),
                            redacted_value: mask_value(&m.value),
                            snippet: redacted_snippet(&target.text, m, &all),
                            at_ms: target.at_ms,
                            occurrences: 1,
                        },
                    );
                }
            }
        }
    }

    let mut findings: Vec<SensitiveFinding> = findings.into_values().collect();
    findings.sort_by(|a, b| {
        a.at_ms
            .unwrap_or(i64::MAX)
            .cmp(&b.at_ms.unwrap_or(i64::MAX))
    });

    let mut counts: HashMap<String, usize> = HashMap::new();
    for finding in &findings {
        *counts
            .entry(category_key(finding.category).to_string())
            .or_insert(0) += finding.occurrences as usize;
    }
    let total: usize = findings.iter().map(|f| f.occurrences as usize).sum();
    let high = findings
        .iter()
        .filter(|f| matches!(f.severity, SensitiveSeverity::High))
        .map(|f| f.occurrences as usize)
        .sum();

    values.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));

    ScanOutcome {
        report: SensitiveReport {
            session_id: session_id.to_string(),
            scanned_at: chrono::Utc::now().timestamp_millis(),
            total_findings: total,
            high_severity_count: high,
            counts,
            findings,
            images: None,
        },
        values,
    }
}

pub struct TextRedactor {
    values: Vec<String>,
}

impl TextRedactor {
    pub fn new(values: Vec<String>) -> Self {
        let mut values = values;
        values.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
        Self { values }
    }

    pub fn passthrough() -> Self {
        Self { values: Vec::new() }
    }

    pub fn redact(&self, text: &str) -> String {
        if self.values.is_empty() {
            return text.to_string();
        }
        let mut out = text.to_string();
        for value in &self.values {
            if out.contains(value.as_str()) {
                out = out.replace(value.as_str(), &mask_value(value));
            }
        }
        out
    }
}

pub fn save_report(dir: &Path, report: Option<&SensitiveReport>) {
    let path = dir.join(SENSITIVE_REPORT_FILE);
    match report {
        Some(report) => {
            if let Ok(bytes) = serde_json::to_vec_pretty(report) {
                let _ = std::fs::write(path, bytes);
            }
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn load_report(dir: &Path) -> Option<SensitiveReport> {
    let content = std::fs::read_to_string(dir.join(SENSITIVE_REPORT_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}
