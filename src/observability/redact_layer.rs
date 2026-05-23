// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt;

use tracing::field::{Field, Visit};
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::FormatFields;

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
    "client_secret",
    "private_key",
    "session_token",
    "x-api-key",
    "x_api_key",
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

#[derive(Debug, Clone)]
pub struct RedactingFieldFormatter {
    sensitive_fields: Vec<String>,
    tail_len: usize,
}

impl Default for RedactingFieldFormatter {
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

impl RedactingFieldFormatter {
    pub fn with_additional_fields(mut self, extra: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for f in extra {
            self.sensitive_fields.push(f.into());
        }
        self
    }

    pub fn with_tail_len(mut self, tail: usize) -> Self {
        self.tail_len = tail;
        self
    }
}

impl<'writer> FormatFields<'writer> for RedactingFieldFormatter {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut visitor = RedactingWriteVisitor {
            writer: &mut writer,
            sensitive: &self.sensitive_fields,
            tail_len: self.tail_len,
            result: Ok(()),
            first: true,
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

struct RedactingWriteVisitor<'a, 'writer> {
    writer: &'a mut Writer<'writer>,
    sensitive: &'a [String],
    tail_len: usize,
    result: fmt::Result,
    first: bool,
}

impl RedactingWriteVisitor<'_, '_> {
    fn should_redact(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.sensitive
            .iter()
            .any(|pattern| lower.contains(&pattern.to_ascii_lowercase()))
    }

    fn write_pair(&mut self, name: &str, value: &str) {
        if self.result.is_err() {
            return;
        }
        let display_value = if self.should_redact(name) {
            redact_impl(value, self.tail_len)
        } else {
            value.to_string()
        };
        let res = if name == "message" {
            if self.first {
                write!(self.writer, "{display_value}")
            } else {
                write!(self.writer, " {display_value}")
            }
        } else if self.first {
            write!(self.writer, "{name}={display_value}")
        } else {
            write!(self.writer, " {name}={display_value}")
        };
        if res.is_ok() {
            self.first = false;
        }
        self.result = res;
    }
}

impl Visit for RedactingWriteVisitor<'_, '_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.write_pair(field.name(), value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.write_pair(field.name(), &format!("{value:?}"));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.write_pair(field.name(), &value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.write_pair(field.name(), &value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.write_pair(field.name(), &value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.write_pair(field.name(), &value.to_string());
    }
}
