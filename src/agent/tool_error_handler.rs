// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tool Error Handler - graceful tool execution error handling.
//!
//! Wraps tool execution to catch panics and errors, returning structured
//! error messages so the agent loop can continue rather than crash.

/// Maximum characters for error detail in the response.
const MAX_ERROR_DETAIL_CHARS: usize = 500;

/// Wraps a tool execution result, converting errors to structured messages.
pub fn handle_tool_error(tool_name: &str, result: Result<String, anyhow::Error>) -> String {
    match result {
        Ok(output) => output,
        Err(err) => {
            let detail = format!("{err:#}");
            let truncated = truncate_error(&detail, MAX_ERROR_DETAIL_CHARS);

            tracing::warn!(
                tool = %tool_name,
                error = %truncated,
                "Tool execution failed, returning error to agent"
            );

            format!(
                "Error executing tool '{tool_name}': {truncated}\n\n\
                 The tool encountered an error. \
                 You may retry with different parameters or use an alternative approach.",
            )
        }
    }
}

/// Format a tool error for inclusion in conversation history.
pub fn format_tool_error(tool_name: &str, tool_call_id: &str, error: &str) -> String {
    let truncated = truncate_error(error, MAX_ERROR_DETAIL_CHARS);
    format!("[Tool Error] {tool_name} (call_id: {tool_call_id}): {truncated}")
}

/// Check if a tool result indicates an error condition.
pub fn is_error_result(result: &str) -> bool {
    let lower = result.to_lowercase();
    lower.starts_with("error")
        || lower.starts_with("[error]")
        || lower.starts_with("[tool error]")
        || lower.contains("traceback (most recent call last)")
        || lower.contains("panicked at")
}

fn truncate_error(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}... [truncated]", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_success() {
        let result = handle_tool_error("shell", Ok("output".to_string()));
        assert_eq!(result, "output");
    }

    #[test]
    fn test_handle_error() {
        let err = anyhow::anyhow!("command not found");
        let result = handle_tool_error("shell", Err(err));
        assert!(result.contains("Error executing tool 'shell'"));
        assert!(result.contains("command not found"));
    }

    #[test]
    fn test_truncation() {
        let long_error = "x".repeat(1000);
        let truncated = truncate_error(&long_error, 100);
        assert!(truncated.len() < 150);
        assert!(truncated.ends_with("... [truncated]"));
    }

    #[test]
    fn test_short_string_not_truncated() {
        let short = "brief error";
        let result = truncate_error(short, 100);
        assert_eq!(result, short);
    }

    #[test]
    fn test_error_detection() {
        assert!(is_error_result("Error: file not found"));
        assert!(is_error_result("[Error] something broke"));
        assert!(is_error_result("[Tool Error] shell: failed"));
        assert!(is_error_result(
            "some output\nTraceback (most recent call last):\n..."
        ));
        assert!(is_error_result("thread 'main' panicked at 'oops'"));
        assert!(!is_error_result("Success: file written"));
        assert!(!is_error_result("The error was in your logic"));
    }

    #[test]
    fn test_format_tool_error() {
        let formatted = format_tool_error("shell", "tc-123", "not found");
        assert!(formatted.contains("shell"));
        assert!(formatted.contains("tc-123"));
        assert!(formatted.contains("not found"));
    }
}
