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

fn escape_js_line_terminators(json: &str) -> String {
    json.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub fn ndjson_safe_stringify<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let json = serde_json::to_string(value)?;
    Ok(escape_js_line_terminators(&json))
}

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

pub fn ndjson_parse<T: serde::de::DeserializeOwned>(line: &str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim())
}
