// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde_json::Value;

pub fn parse_error_feedback(tool_name: &str, schema: Option<&Value>) -> String {
    let mut msg = format!(
        "Tool call rejected before execution: the JSON arguments for '{tool_name}' could not \
         be parsed (they were truncated or malformed), so the call was NOT executed. \
         Re-emit the tool call with complete, valid JSON arguments."
    );
    if let Some(excerpt) = schema_excerpt(schema) {
        msg.push_str("\nExpected argument schema:\n");
        msg.push_str(&excerpt);
    }
    msg
}

pub fn validate_args_against_schema(
    tool_name: &str,
    schema: &Value,
    args: &Value,
) -> Option<String> {
    let schema_obj = schema.as_object()?;
    if schema_obj.get("type").and_then(Value::as_str) != Some("object") {
        return None;
    }

    // Defer to the tool's own validation for schemas using composition or
    // references we do not fully model here. Enforcing missing-required against
    // these risks false rejections of otherwise-valid calls (common in MCP
    // tool schemas), which is worse than passing a bad call through.
    const UNSUPPORTED_KEYWORDS: &[&str] =
        &["anyOf", "oneOf", "allOf", "$ref", "if", "then", "else", "not", "dependentRequired"];
    if UNSUPPORTED_KEYWORDS
        .iter()
        .any(|k| schema_obj.contains_key(*k))
    {
        return None;
    }

    let properties = schema_obj.get("properties").and_then(Value::as_object);

    let Some(args_obj) = args.as_object() else {
        return Some(format!(
            "Tool call rejected before execution: '{tool_name}' expects a JSON object of \
             arguments but received {}. Re-emit the call with an argument object.{}",
            json_type_name(args),
            schema_excerpt(Some(schema))
                .map(|s| format!("\nExpected argument schema:\n{s}"))
                .unwrap_or_default()
        ));
    };

    let mut missing: Vec<&str> = Vec::new();
    if let Some(required) = schema_obj.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            let present = args_obj
                .get(key)
                .map(|v| !v.is_null())
                .unwrap_or(false);
            if !present {
                missing.push(key);
            }
        }
    }

    // Only flag STRUCTURALLY incompatible type mismatches — passing an
    // array/object where a scalar is expected, or vice versa. Scalar<->string
    // interchange (e.g. "5" for an integer, or a number where a string is
    // expected) is deliberately tolerated: models emit these constantly and
    // virtually every tool coerces them, so rejecting would break valid calls.
    let mut type_errors: Vec<String> = Vec::new();
    if let Some(props) = properties {
        for (key, value) in args_obj {
            let Some(decl) = props.get(key) else { continue };
            let Some(expected) = decl.get("type").and_then(Value::as_str) else {
                continue;
            };
            if value.is_null() {
                continue;
            }
            let structurally_ok = match expected {
                "array" => value.is_array(),
                "object" => value.is_object(),
                // Scalars: reject only if the model sent a composite (array or
                // object) where a scalar was expected; accept any scalar form.
                "string" | "boolean" | "integer" | "number" => {
                    !value.is_array() && !value.is_object()
                }
                _ => true,
            };
            if !structurally_ok {
                type_errors.push(format!(
                    "'{key}' should be {expected} but got {}",
                    json_type_name(value)
                ));
            }
        }
    }

    if missing.is_empty() && type_errors.is_empty() {
        return None;
    }

    let mut msg = format!(
        "Tool call rejected before execution: invalid arguments for '{tool_name}'."
    );
    if !missing.is_empty() {
        msg.push_str(&format!("\nMissing required: {}", missing.join(", ")));
    }
    if !type_errors.is_empty() {
        msg.push_str(&format!("\nType mismatches: {}", type_errors.join("; ")));
    }
    if let Some(excerpt) = schema_excerpt(Some(schema)) {
        msg.push_str("\nExpected argument schema:\n");
        msg.push_str(&excerpt);
    }
    msg.push_str("\nRe-emit the tool call with corrected arguments. The tool was NOT executed.");
    Some(msg)
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn schema_excerpt(schema: Option<&Value>) -> Option<String> {
    let schema = schema?;
    let rendered = serde_json::to_string_pretty(schema).ok()?;
    const MAX: usize = 1_600;
    if rendered.len() <= MAX {
        return Some(rendered);
    }
    let mut end = MAX;
    while end > 0 && !rendered.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{}\u{2026} (schema truncated)", &rendered[..end]))
}
