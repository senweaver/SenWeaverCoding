// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Safe NDJSON serialization.
//!
//! JSON.stringify emits U+2028/U+2029 raw (valid per ECMA-404 / RFC 8259).
//! When the output is a single NDJSON line, any receiver that uses JavaScript
//! line-terminator semantics to split the stream will cut the JSON mid-string.
//! The `\uXXXX` escape form is equivalent JSON but can never be mistaken for
//! a line terminator by any receiver.

use serde::Serialize;

/// Escape JS line terminators (U+2028 LINE SEPARATOR, U+2029 PARAGRAPH
/// SEPARATOR) so the serialized output cannot be broken by a line-splitting
/// receiver.
fn escape_js_line_terminators(json: &str) -> String {
    json.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Serialize a value to a single NDJSON-safe line.
///
/// Escapes U+2028 and U+2029 after JSON serialization so the output is
/// safe for one-message-per-line transports.
pub fn ndjson_safe_stringify<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let json = serde_json::to_string(value)?;
    Ok(escape_js_line_terminators(&json))
}

/// Write a single NDJSON message to stdout followed by a newline.
pub fn write_ndjson_stdout<T: Serialize>(value: &T) -> std::io::Result<()> {
    use std::io::Write;
    let line = ndjson_safe_stringify(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(line.as_bytes())?;
    handle.write_all(b"\n")?;
    handle.flush()
}

/// Parse a single NDJSON line into a deserialized value.
pub fn ndjson_parse<T: serde::de::DeserializeOwned>(line: &str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn escapes_line_separator() {
        let input = "hello\u{2028}world";
        assert_eq!(escape_js_line_terminators(input), "hello\\u2028world");
    }

    #[test]
    fn escapes_paragraph_separator() {
        let input = "hello\u{2029}world";
        assert_eq!(escape_js_line_terminators(input), "hello\\u2029world");
    }

    #[test]
    fn no_escape_needed() {
        let input = r#"{"key":"value"}"#;
        assert_eq!(escape_js_line_terminators(input), input);
    }

    #[test]
    fn stringify_simple_value() {
        let val = json!({"name": "test", "count": 42});
        let result = ndjson_safe_stringify(&val).unwrap();
        assert!(!result.contains('\n'));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["name"], "test");
    }

    #[test]
    fn parse_ndjson_line() {
        let line = r#"{"type":"message","content":"hello"}"#;
        let val: serde_json::Value = ndjson_parse(line).unwrap();
        assert_eq!(val["type"], "message");
    }

    #[test]
    fn parse_with_trailing_whitespace() {
        let line = r#"{"a":1}  "#;
        let val: serde_json::Value = ndjson_parse(line).unwrap();
        assert_eq!(val["a"], 1);
    }
}
