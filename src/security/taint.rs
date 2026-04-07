// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Taint tracking system for data flow integrity.
//!
//! This module provides security taint tracking:
//! - Labels for data sources (external network, user input, PII, secrets)
//! - Tainted values that carry their taint labels through the system
//! - Sink checking before sensitive operations (shell, network, agent messages)
//! - Declassification and cleaning operations for sanitization
//!
//! Taint tracking helps prevent injection attacks and data exfiltration by
//! tracking where data came from and validating it's safe before use.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// A taint label indicating the source or sensitivity of data.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaintLabel {
    /// Data from external network sources (untrusted).
    ExternalNetwork,
    /// Data from user input (could be malicious).
    UserInput,
    /// Personally identifiable information (sensitive).
    Pii,
    /// Secrets like passwords, tokens, keys.
    Secret,
    /// Data from untrusted agents (in multi-agent systems).
    UntrustedAgent,
    /// Data that has been sanitized/cleaned.
    Clean,
}

impl std::fmt::Display for TaintLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// A value carrying taint labels indicating its source and sensitivity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaintedValue {
    /// The actual data value (as string for most use cases).
    pub value: String,
    /// Set of taint labels applied to this value.
    pub labels: HashSet<TaintLabel>,
    /// Source origin description (e.g., "http://example.com", "user_input_field").
    pub source: String,
}

impl TaintedValue {
    /// Create a new tainted value with specified labels.
    pub fn new(
        value: impl Into<String>,
        labels: Vec<TaintLabel>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            value: value.into(),
            labels: labels.into_iter().collect(),
            source: source.into(),
        }
    }

    /// Create a clean value with no taint labels.
    pub fn clean(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            value: value.clone(),
            labels: HashSet::new(),
            source: "internal".to_string(),
        }
    }

    /// Create a value tainted with external network source.
    pub fn from_network(value: impl Into<String>, source: impl Into<String>) -> Self {
        Self::new(value, vec![TaintLabel::ExternalNetwork], source)
    }

    /// Create a value tainted with user input.
    pub fn from_user(value: impl Into<String>, field: impl Into<String>) -> Self {
        Self::new(
            value,
            vec![TaintLabel::UserInput],
            format!("user_input:{}", field.into()),
        )
    }

    /// Create a value containing PII.
    pub fn pii(value: impl Into<String>, source: impl Into<String>) -> Self {
        Self::new(value, vec![TaintLabel::Pii], source)
    }

    /// Create a value containing secrets.
    pub fn secret(value: impl Into<String>, source: impl Into<String>) -> Self {
        Self::new(value, vec![TaintLabel::Secret], source)
    }

    /// Check if this value has the specified taint label.
    pub fn has_label(&self, label: TaintLabel) -> bool {
        self.labels.contains(&label)
    }

    /// Check if this value has any taint labels.
    pub fn is_tainted(&self) -> bool {
        !self.labels.is_empty() && !self.has_label(TaintLabel::Clean)
    }

    /// Check if this value has any of the specified labels.
    pub fn has_any_label(&self, labels: &[TaintLabel]) -> bool {
        labels.iter().any(|l| self.labels.contains(l))
    }

    /// Add a taint label to this value.
    pub fn add_label(&mut self, label: TaintLabel) {
        self.labels.insert(label);
    }

    /// Remove a taint label (use sparingly, prefer declassification).
    pub fn remove_label(&mut self, label: TaintLabel) {
        self.labels.remove(&label);
    }

    /// Merge taint labels from another tainted value.
    /// Used when combining data from multiple sources.
    pub fn merge_taint(&mut self, other: &TaintedValue) {
        self.labels.extend(other.labels.iter().cloned());
        // Update source to reflect merge
        if self.source != other.source {
            self.source = format!("{} + {}", self.source, other.source);
        }
    }

    /// Create a new value combining taint from multiple sources.
    pub fn merge_multiple(values: &[&TaintedValue], combined_value: impl Into<String>) -> Self {
        let mut result = Self::clean(combined_value);
        for v in values {
            result.merge_taint(v);
        }
        result
    }

    /// Declassify by removing specific taint labels.
    /// Returns a new value with labels removed (original unchanged).
    pub fn declassify(&self, labels_to_remove: &[TaintLabel]) -> Self {
        let mut new_labels = self.labels.clone();
        for label in labels_to_remove {
            new_labels.remove(label);
        }
        Self {
            value: self.value.clone(),
            labels: new_labels,
            source: format!("{} (declassified)", self.source),
        }
    }

    /// Mark as clean (sanitized/validated).
    /// This replaces all labels with the Clean label.
    pub fn clean_sanitized(&self) -> Self {
        Self {
            value: self.value.clone(),
            labels: HashSet::from([TaintLabel::Clean]),
            source: format!("{} (sanitized)", self.source),
        }
    }

    /// Extract the raw value (use with caution - loses taint info).
    pub fn into_value(self) -> String {
        self.value
    }

    /// Get the value as a reference.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Get the taint labels as a formatted string.
    pub fn labels_string(&self) -> String {
        let labels: Vec<_> = self.labels.iter().map(|l| format!("{:?}", l)).collect();
        labels.join(", ")
    }
}

impl From<String> for TaintedValue {
    fn from(value: String) -> Self {
        Self::clean(value)
    }
}

impl From<&str> for TaintedValue {
    fn from(value: &str) -> Self {
        Self::clean(value)
    }
}

/// A sink where tainted data could cause security issues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaintSink {
    /// Name of the sink operation.
    pub name: String,
    /// Labels that are blocked from entering this sink.
    pub blocked_labels: HashSet<TaintLabel>,
    /// Description of what this sink does.
    pub description: String,
}

impl TaintSink {
    /// Create a new taint sink.
    pub fn new(
        name: impl Into<String>,
        blocked: Vec<TaintLabel>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            blocked_labels: blocked.into_iter().collect(),
            description: description.into(),
        }
    }

    /// Check if a value can safely enter this sink.
    pub fn check(&self, value: &TaintedValue) -> Result<(), TaintViolation> {
        let blocked: Vec<_> = value.labels.intersection(&self.blocked_labels).collect();

        if blocked.is_empty() {
            Ok(())
        } else {
            Err(TaintViolation {
                labels: blocked.into_iter().cloned().collect(),
                sink_name: self.name.clone(),
                data_source: value.source.clone(),
                value_preview: if value.has_any_label(&[TaintLabel::Secret, TaintLabel::Pii]) {
                    "[REDACTED]".to_string()
                } else if value.value.len() > 50 {
                    format!("{}...", &value.value[..50])
                } else {
                    value.value.clone()
                },
            })
        }
    }

    /// Shell execution sink - blocks user input and external network data.
    pub fn shell_exec() -> Self {
        Self::new(
            "shell_exec",
            vec![TaintLabel::UserInput, TaintLabel::ExternalNetwork],
            "Execute shell command",
        )
    }

    /// Network fetch sink - blocks secrets and PII from being sent out.
    pub fn net_fetch() -> Self {
        Self::new(
            "net_fetch",
            vec![TaintLabel::Secret, TaintLabel::Pii],
            "Fetch from network",
        )
    }

    /// Agent message sink - blocks secrets from being sent to untrusted agents.
    pub fn agent_message() -> Self {
        Self::new(
            "agent_message",
            vec![TaintLabel::Secret],
            "Send message to agent",
        )
    }

    /// File write sink - blocks external network data from being written.
    pub fn file_write() -> Self {
        Self::new(
            "file_write",
            vec![TaintLabel::ExternalNetwork, TaintLabel::UntrustedAgent],
            "Write to file",
        )
    }
}

/// A taint violation occurred when checking a sink.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub struct TaintViolation {
    /// The taint labels that caused the violation.
    pub labels: Vec<TaintLabel>,
    /// The sink that was being accessed.
    pub sink_name: String,
    /// The source of the tainted data.
    pub data_source: String,
    /// Preview of the value that was blocked.
    pub value_preview: String,
}

impl std::fmt::Display for TaintViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Taint violation: data with labels [{}] from source '{}' cannot enter sink '{}' (value preview: {})",
            self.labels
                .iter()
                .map(|l| format!("{:?}", l))
                .collect::<Vec<_>>()
                .join(", "),
            self.data_source,
            self.sink_name,
            self.value_preview
        )
    }
}

impl TaintViolation {
    /// Get the labels as a formatted string.
    pub fn labels_string(&self) -> String {
        self.labels
            .iter()
            .map(|l| format!("{:?}", l))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Extension trait for Option<TaintedValue>.
pub trait TaintedOptionExt {
    /// Check if the value (if present) is safe for the given sink.
    fn check_sink(&self, sink: &TaintSink) -> Result<(), TaintViolation>;
}

impl TaintedOptionExt for Option<TaintedValue> {
    fn check_sink(&self, sink: &TaintSink) -> Result<(), TaintViolation> {
        match self {
            Some(value) => sink.check(value),
            None => Ok(()),
        }
    }
}

/// Extension trait for Result with tainted values.
pub trait TaintedResultExt {
    /// Check the tainted value in the result against a sink.
    fn check_sink(self, sink: &TaintSink) -> Self;
}

/// Sanitization functions for cleaning tainted data.
pub mod sanitizers {
    use super::TaintedValue;

    /// Basic string sanitizer that removes shell metacharacters.
    pub fn sanitize_shell(value: &str) -> String {
        value
            .replace(';', "")
            .replace('&', "")
            .replace('|', "")
            .replace('$', "")
            .replace('`', "")
            .replace('(', "")
            .replace(')', "")
            .replace('<', "")
            .replace('>', "")
    }

    /// URL sanitizer that ensures valid URL format.
    pub fn sanitize_url(value: &str) -> Option<String> {
        if value.starts_with("http://") || value.starts_with("https://") {
            Some(value.to_string())
        } else {
            None
        }
    }

    /// Apply a sanitizer and return a cleaned tainted value.
    pub fn apply<F>(value: &TaintedValue, sanitizer: F, operation: &str) -> TaintedValue
    where
        F: FnOnce(&str) -> String,
    {
        let cleaned = sanitizer(&value.value);
        TaintedValue {
            value: cleaned,
            labels: value.labels.clone(),
            source: format!("{} (sanitized: {})", value.source, operation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tainted_value_creation() {
        let clean = TaintedValue::clean("hello");
        assert!(!clean.is_tainted());

        let from_net = TaintedValue::from_network("data", "api.example.com");
        assert!(from_net.is_tainted());
        assert!(from_net.has_label(TaintLabel::ExternalNetwork));

        let user_input = TaintedValue::from_user("input", "search_box");
        assert!(user_input.has_label(TaintLabel::UserInput));
    }

    #[test]
    fn test_taint_merge() {
        let mut a = TaintedValue::from_network("a", "api1.com");
        let b = TaintedValue::from_user("b", "form");

        a.merge_taint(&b);

        assert!(a.has_label(TaintLabel::ExternalNetwork));
        assert!(a.has_label(TaintLabel::UserInput));
    }

    #[test]
    fn test_taint_merge_multiple() {
        let a = TaintedValue::from_network("a", "api.com");
        let b = TaintedValue::from_user("b", "form");
        let c = TaintedValue::pii("c", "database");

        let merged = TaintedValue::merge_multiple(&[&a, &b, &c], "combined");

        assert!(merged.has_label(TaintLabel::ExternalNetwork));
        assert!(merged.has_label(TaintLabel::UserInput));
        assert!(merged.has_label(TaintLabel::Pii));
    }

    #[test]
    fn test_declassify() {
        let tainted = TaintedValue::from_network("data", "api.com");
        assert!(tainted.has_label(TaintLabel::ExternalNetwork));

        let declassified = tainted.declassify(&[TaintLabel::ExternalNetwork]);
        assert!(!declassified.has_label(TaintLabel::ExternalNetwork));
    }

    #[test]
    fn test_clean_sanitized() {
        let tainted = TaintedValue::from_network("data", "api.com");
        let cleaned = tainted.clean_sanitized();

        assert!(cleaned.has_label(TaintLabel::Clean));
        assert!(!cleaned.has_label(TaintLabel::ExternalNetwork));
    }

    #[test]
    fn test_sink_check_passes() {
        let shell_sink = TaintSink::shell_exec();
        let clean = TaintedValue::clean("ls -la");

        assert!(shell_sink.check(&clean).is_ok());
    }

    #[test]
    fn test_sink_check_fails() {
        let shell_sink = TaintSink::shell_exec();
        let user_input = TaintedValue::from_user("rm -rf /", "command_box");

        let result = shell_sink.check(&user_input);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.sink_name.contains("shell"));
        assert!(err.labels.contains(&TaintLabel::UserInput));
    }

    #[test]
    fn test_net_fetch_blocks_secrets() {
        let net_sink = TaintSink::net_fetch();
        let secret = TaintedValue::secret("api_key_123", "config");

        let result = net_sink.check(&secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_shell_sanitizer() {
        let dangerous = "; rm -rf /";
        let cleaned = sanitizers::sanitize_shell(dangerous);
        assert!(!cleaned.contains(';'));
        assert!(!cleaned.contains("rm -rf"));
    }

    #[test]
    fn test_url_sanitizer() {
        assert!(sanitizers::sanitize_url("https://example.com").is_some());
        assert!(sanitizers::sanitize_url("ftp://example.com").is_none());
        assert!(sanitizers::sanitize_url("javascript:alert(1)").is_none());
    }

    #[test]
    fn test_tainted_option_ext() {
        let sink = TaintSink::shell_exec();
        let clean: Option<TaintedValue> = Some(TaintedValue::clean("ls"));
        let tainted: Option<TaintedValue> = Some(TaintedValue::from_user("rm", "input"));
        let none: Option<TaintedValue> = None;

        assert!(clean.check_sink(&sink).is_ok());
        assert!(tainted.check_sink(&sink).is_err());
        assert!(none.check_sink(&sink).is_ok()); // None is always safe
    }

    #[test]
    fn test_taint_labels_string() {
        let value = TaintedValue::new("test", vec![TaintLabel::Pii, TaintLabel::Secret], "source");
        let labels_str = value.labels_string();
        assert!(labels_str.contains("Pii") || labels_str.contains("Secret"));
    }

    #[test]
    fn test_taint_display() {
        let label = TaintLabel::ExternalNetwork;
        let s = format!("{}", label);
        assert_eq!(s, "ExternalNetwork");
    }
}
