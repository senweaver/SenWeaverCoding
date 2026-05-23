// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::providers::ChatMessage;
use regex::Regex;
use std::sync::LazyLock;

static SENSITIVE_KV_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(token|api[_-]?key|password|secret|user[_-]?key|bearer|credential)["']?\s*[:=]\s*(?:"([^"]{8,})"|'([^']{8,})'|([a-zA-Z0-9_\-\.]{8,}))"#)
        .expect("sensitive key/value regex must compile")
});

pub(crate) fn pii_sanitize_text_outside_image_markers(
    input: &str,
) -> (String, crate::services::pii_sanitizer::SanitizationReport) {
    let mut combined =
        crate::services::pii_sanitizer::SanitizationReport::default();
    if input.is_empty() {
        return (String::new(), combined);
    }

    let marker = "[IMAGE:";
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0_usize;

    while let Some(rel) = input[cursor..].find(marker) {
        let start = cursor + rel;
        let prefix = &input[cursor..start];
        if !prefix.is_empty() {
            let (clean, report) = crate::services::pii_sanitizer::sanitize_text(prefix);
            combined.merge(&report);
            output.push_str(&clean);
        }

        let marker_open = start + marker.len();
        if let Some(rel_end) = input[marker_open..].find(']') {
            let end = marker_open + rel_end;
            output.push_str(&input[start..=end]);
            cursor = end + 1;
        } else {
            output.push_str(&input[start..]);
            cursor = input.len();
            break;
        }
    }

    if cursor < input.len() {
        let (clean, report) = crate::services::pii_sanitizer::sanitize_text(&input[cursor..]);
        combined.merge(&report);
        output.push_str(&clean);
    }

    (output, combined)
}

fn pii_sanitize_json_value_image_aware(
    value: &serde_json::Value,
    report: &mut crate::services::pii_sanitizer::SanitizationReport,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if s.contains("[IMAGE:") {
                let (clean, sub) = pii_sanitize_text_outside_image_markers(s);
                report.merge(&sub);
                serde_json::Value::String(clean)
            } else {
                let (clean, sub) = crate::services::pii_sanitizer::sanitize_text(s);
                report.merge(&sub);
                serde_json::Value::String(clean)
            }
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|item| pii_sanitize_json_value_image_aware(item, report))
                .collect(),
        ),
        serde_json::Value::Object(obj) => {
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (k, v) in obj.iter() {
                out.insert(
                    k.clone(),
                    pii_sanitize_json_value_image_aware(v, report),
                );
            }
            serde_json::Value::Object(out)
        }
        other => other.clone(),
    }
}

fn pii_sanitize_assistant_content(
    content: &str,
    report: &mut crate::services::pii_sanitizer::SanitizationReport,
) -> String {
    let trimmed = content.trim_start();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        let (clean, sub) = pii_sanitize_text_outside_image_markers(content);
        report.merge(&sub);
        return clean;
    }

    let mut value = match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v) => v,
        Err(_) => {
            let (clean, sub) = pii_sanitize_text_outside_image_markers(content);
            report.merge(&sub);
            return clean;
        }
    };

    let object = match value.as_object_mut() {
        Some(obj) => obj,
        None => {
            let cleaned = pii_sanitize_json_value_image_aware(&value, report);
            return cleaned.to_string();
        }
    };

    let tool_calls_extracted = object.remove("tool_calls");

    for (_key, slot) in object.iter_mut() {
        let new_value = pii_sanitize_json_value_image_aware(slot, report);
        *slot = new_value;
    }

    if let Some(serde_json::Value::Array(mut calls)) = tool_calls_extracted {
        for call in calls.iter_mut() {
            if let Some(call_obj) = call.as_object_mut() {
                let arguments_extracted = call_obj.remove("arguments");
                for (_k, v) in call_obj.iter_mut() {
                    let new_v = pii_sanitize_json_value_image_aware(v, report);
                    *v = new_v;
                }
                if let Some(args) = arguments_extracted {
                    let sanitized_args = match args {
                        serde_json::Value::String(args_str) => {
                            if let Ok(parsed) =
                                serde_json::from_str::<serde_json::Value>(&args_str)
                            {
                                let cleaned =
                                    pii_sanitize_json_value_image_aware(&parsed, report);
                                serde_json::Value::String(cleaned.to_string())
                            } else {
                                let (clean, sub) =
                                    pii_sanitize_text_outside_image_markers(&args_str);
                                report.merge(&sub);
                                serde_json::Value::String(clean)
                            }
                        }
                        other => pii_sanitize_json_value_image_aware(&other, report),
                    };
                    call_obj.insert("arguments".to_string(), sanitized_args);
                }
            } else {
                let cleaned = pii_sanitize_json_value_image_aware(call, report);
                *call = cleaned;
            }
        }
        object.insert("tool_calls".to_string(), serde_json::Value::Array(calls));
    }

    value.to_string()
}

fn pii_sanitize_tool_envelope(
    content: &str,
    report: &mut crate::services::pii_sanitizer::SanitizationReport,
) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with('{') {
        let (clean, sub) = pii_sanitize_text_outside_image_markers(content);
        report.merge(&sub);
        return clean;
    }

    let mut value = match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v) => v,
        Err(_) => {
            let (clean, sub) = pii_sanitize_text_outside_image_markers(content);
            report.merge(&sub);
            return clean;
        }
    };

    if value.as_object_mut().is_some() {
        let cleaned = pii_sanitize_json_value_image_aware(&value, report);
        return cleaned.to_string();
    }

    let cleaned = pii_sanitize_json_value_image_aware(&value, report);
    cleaned.to_string()
}

pub(crate) fn apply_outgoing_pii_sanitization(
    coding_mode: Option<crate::agent::coding_mode::CodingMode>,
    messages: &mut [ChatMessage],
) -> crate::services::pii_sanitizer::SanitizationReport {
    let mut report = crate::services::pii_sanitizer::SanitizationReport::default();
    if !matches!(coding_mode, Some(crate::agent::coding_mode::CodingMode::Debug)) {
        return report;
    }
    if !crate::services::pii_sanitizer::global_sanitizer().enabled() {
        return report;
    }

    for msg in messages.iter_mut() {
        let original = std::mem::take(&mut msg.content);
        let cleaned = match msg.role.as_str() {
            "assistant" => pii_sanitize_assistant_content(&original, &mut report),
            "tool" => pii_sanitize_tool_envelope(&original, &mut report),
            _ => {
                let (clean, sub) = pii_sanitize_text_outside_image_markers(&original);
                report.merge(&sub);
                clean
            }
        };
        msg.content = cleaned;
    }

    report
}

pub(crate) fn scrub_credentials(input: &str) -> String {
    let after_vault =
        crate::services::credential_vault::redact_for_audit_optional(input);
    SENSITIVE_KV_REGEX
        .replace_all(&after_vault, |caps: &regex::Captures| {
            let full_match = &caps[0];
            let key = &caps[1];
            let val = caps
                .get(2)
                .or(caps.get(3))
                .or(caps.get(4))
                .map(|m| m.as_str())
                .unwrap_or("");

            let prefix = if val.len() > 4 {
                val.char_indices()
                    .nth(4)
                    .map(|(byte_idx, _)| &val[..byte_idx])
                    .unwrap_or(val)
            } else {
                ""
            };

            if full_match.contains(':') {
                if full_match.contains('"') {
                    format!("\"{}\": \"{}*[REDACTED]\"", key, prefix)
                } else {
                    format!("{}: {}*[REDACTED]", key, prefix)
                }
            } else if full_match.contains('=') {
                if full_match.contains('"') {
                    format!("{}=\"{}*[REDACTED]\"", key, prefix)
                } else {
                    format!("{}={}*[REDACTED]", key, prefix)
                }
            } else {
                format!("{}: {}*[REDACTED]", key, prefix)
            }
        })
        .to_string()
}
