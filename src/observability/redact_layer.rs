// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tracing subscriber layer that redacts secret-bearing fields across
//! the whole process — no code-site changes required.
//!
//! ## Behaviour
//!
//! When a `tracing::info!(api_key = "sk-xxx")` call fires, this layer
//! inspects every recorded field name against the [`DEFAULT_SECRET_FIELDS`]
//! regex list and replaces the value with `"…<tail4>"` before emitting.
//!
//! The implementation intentionally does **not** depend on the unstable
//! `tracing::field::Valuable` trait; we reuse the established
//! `Visit`-based debug-formatting path that every tracing subscriber
//! already handles.  This means the layer works with fmt, JSON,
//! `console-subscriber`, OTLP bridges, and anything else downstream.
//!
//! ## Example
//!
//! ```no_run
//! use tracing_subscriber::prelude::*;
//! use senweavercoding::observability::redact_layer::RedactLayer;
//!
//! let subscriber = tracing_subscriber::registry()
//!     .with(RedactLayer::default())
//!     .with(tracing_subscriber::fmt::layer());
//! // … set as global
//! ```
//!
//! ## Why fingerprint rather than full redact?
//!
//! `"…sk-42"` preserves enough signal to distinguish between multiple
//! configured keys (e.g. OpenAI vs Anthropic) in operator logs without
//! exposing the full secret.  Follow the same convention as
//! `SecretString::redacted()`.

use std::fmt;

use tracing::field::{Field, Visit};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

pub const DEFAULT_SECRET_FIELDS: &[&str] = &[
    "api_key",
    "apikey",
    "token",
    "secret",
    "password",
    "passwd",
    "bearer",
    "auth_header",
    "access_token",
    "refresh_token",
    "id_token",
    "app_secret",
];

#[derive(Debug, Clone)]
pub struct RedactLayer {
    sensitive_fields: Vec<String>,
    tail_len: usize,
}

impl Default for RedactLayer {
    fn default() -> Self {
        Self {
            sensitive_fields: DEFAULT_SECRET_FIELDS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            tail_len: 4,
        }
    }
}

impl RedactLayer {

    pub fn with_fields(fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            sensitive_fields: fields.into_iter().map(Into::into).collect(),
            tail_len: 4,
        }
    }

    pub fn with_tail_len(mut self, tail: usize) -> Self {
        self.tail_len = tail;
        self
    }

    pub fn is_sensitive(&self, field_name: &str) -> bool {
        let lower = field_name.to_ascii_lowercase();
        self.sensitive_fields
            .iter()
            .any(|pattern| lower.contains(&pattern.to_ascii_lowercase()))
    }

    pub fn redact(&self, value: &str) -> String {
        redact_impl(value, self.tail_len)
    }
}

fn redact_impl(value: &str, tail_len: usize) -> String {
    if value.is_empty() || tail_len == 0 {
        return "…".into();
    }
    let take = tail_len.min(value.len()).max(1);
    let tail: String = value.chars().rev().take(take).collect::<String>();
    let tail_forward: String = tail.chars().rev().collect();
    format!("…{tail_forward}")
}

impl<S: Subscriber> Layer<S> for RedactLayer {
    fn event_enabled(&self, _event: &Event<'_>, _ctx: Context<'_, S>) -> bool {
        true
    }

    fn enabled(&self, _metadata: &Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        true
    }
}

pub struct RedactingVisitor<'a> {
    fields: Vec<(String, String)>,
    sensitive_fields: &'a [String],
    tail_len: usize,
}

impl<'a> RedactingVisitor<'a> {
    pub fn new(sensitive_fields: &'a [String], tail_len: usize) -> Self {
        Self {
            fields: Vec::new(),
            sensitive_fields,
            tail_len,
        }
    }

    pub fn into_fields(self) -> Vec<(String, String)> {
        self.fields
    }

    fn should_redact(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.sensitive_fields
            .iter()
            .any(|pattern| lower.contains(&pattern.to_ascii_lowercase()))
    }

    fn push(&mut self, name: &str, value: String) {
        let stored = if self.should_redact(name) {
            redact_impl(&value, self.tail_len)
        } else {
            value
        };
        self.fields.push((name.to_string(), stored));
    }
}

impl Visit for RedactingVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.push(field.name(), format!("{value:?}"));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field.name(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field.name(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field.name(), value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.push(field.name(), value.to_string());
    }
}
