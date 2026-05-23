// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaintLabel {

    ExternalNetwork,

    UserInput,

    Pii,

    Secret,

    UntrustedAgent,

    Clean,
}

impl std::fmt::Display for TaintLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaintedValue {

    pub value: String,

    pub labels: HashSet<TaintLabel>,

    pub source: String,
}

impl TaintedValue {

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

    pub fn clean(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            value: value.clone(),
            labels: HashSet::new(),
            source: "internal".to_string(),
        }
    }

    pub fn from_network(value: impl Into<String>, source: impl Into<String>) -> Self {
        Self::new(value, vec![TaintLabel::ExternalNetwork], source)
    }

    pub fn from_user(value: impl Into<String>, field: impl Into<String>) -> Self {
        Self::new(
            value,
            vec![TaintLabel::UserInput],
            format!("user_input:{}", field.into()),
        )
    }

    pub fn pii(value: impl Into<String>, source: impl Into<String>) -> Self {
        Self::new(value, vec![TaintLabel::Pii], source)
    }

    pub fn secret(value: impl Into<String>, source: impl Into<String>) -> Self {
        Self::new(value, vec![TaintLabel::Secret], source)
    }

    pub fn has_label(&self, label: TaintLabel) -> bool {
        self.labels.contains(&label)
    }

    pub fn is_tainted(&self) -> bool {
        !self.labels.is_empty() && !self.has_label(TaintLabel::Clean)
    }

    pub fn has_any_label(&self, labels: &[TaintLabel]) -> bool {
        labels.iter().any(|l| self.labels.contains(l))
    }

    pub fn add_label(&mut self, label: TaintLabel) {
        self.labels.insert(label);
    }

    pub fn remove_label(&mut self, label: TaintLabel) {
        self.labels.remove(&label);
    }

    pub fn merge_taint(&mut self, other: &TaintedValue) {
        self.labels.extend(other.labels.iter().copied());

        if self.source != other.source {
            self.source = format!("{} + {}", self.source, other.source);
        }
    }

    pub fn merge_multiple(values: &[&TaintedValue], combined_value: impl Into<String>) -> Self {
        let mut result = Self::clean(combined_value);
        for v in values {
            result.merge_taint(v);
        }
        result
    }

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

    pub fn clean_sanitized(&self) -> Self {
        Self {
            value: self.value.clone(),
            labels: HashSet::from([TaintLabel::Clean]),
            source: format!("{} (sanitized)", self.source),
        }
    }

    pub fn into_value(self) -> String {
        self.value
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaintSink {

    pub name: String,

    pub blocked_labels: HashSet<TaintLabel>,

    pub description: String,
}

impl TaintSink {

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

    pub fn check(&self, value: &TaintedValue) -> Result<(), TaintViolation> {
        let blocked: Vec<_> = value.labels.intersection(&self.blocked_labels).collect();

        if blocked.is_empty() {
            Ok(())
        } else {
            Err(TaintViolation {
                labels: blocked.into_iter().copied().collect(),
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

    pub fn shell_exec() -> Self {
        Self::new(
            "shell_exec",
            vec![TaintLabel::UserInput, TaintLabel::ExternalNetwork],
            "Execute shell command",
        )
    }

    pub fn net_fetch() -> Self {
        Self::new(
            "net_fetch",
            vec![TaintLabel::Secret, TaintLabel::Pii],
            "Fetch from network",
        )
    }

    pub fn agent_message() -> Self {
        Self::new(
            "agent_message",
            vec![TaintLabel::Secret],
            "Send message to agent",
        )
    }

    pub fn file_write() -> Self {
        Self::new(
            "file_write",
            vec![TaintLabel::ExternalNetwork, TaintLabel::UntrustedAgent],
            "Write to file",
        )
    }
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub struct TaintViolation {

    pub labels: Vec<TaintLabel>,

    pub sink_name: String,

    pub data_source: String,

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

    pub fn labels_string(&self) -> String {
        self.labels
            .iter()
            .map(|l| format!("{:?}", l))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub trait TaintedOptionExt {

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

pub trait TaintedResultExt {

    fn check_sink(self, sink: &TaintSink) -> Self;
}

pub mod sanitizers {
    use super::TaintedValue;

    pub fn sanitize_shell(value: &str) -> String {
        value.replace([';', '&', '|', '$', '`', '(', ')', '<', '>'], "")
    }

    pub fn sanitize_url(value: &str) -> Option<String> {
        if value.starts_with("http://") || value.starts_with("https://") {
            Some(value.to_string())
        } else {
            None
        }
    }

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
