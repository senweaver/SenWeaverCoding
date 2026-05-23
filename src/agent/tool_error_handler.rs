// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const MAX_ERROR_DETAIL_CHARS: usize = 500;

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

pub fn format_tool_error(tool_name: &str, tool_call_id: &str, error: &str) -> String {
    let truncated = truncate_error(error, MAX_ERROR_DETAIL_CHARS);
    format!("[Tool Error] {tool_name} (call_id: {tool_call_id}): {truncated}")
}

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
