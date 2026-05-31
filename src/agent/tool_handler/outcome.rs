// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::tools::ToolResult;

pub struct NormalizedToolOutcome {
    pub output: String,
    pub success: bool,
    pub error_reason: Option<String>,
}

pub fn is_command_execution_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "shell" | "powershell" | "run_terminal_cmd" | "execute_command"
    )
}

pub fn normalize_tool_result(tool_name: &str, result: ToolResult) -> NormalizedToolOutcome {
    if result.success {
        return NormalizedToolOutcome {
            output: result.output,
            success: true,
            error_reason: None,
        };
    }

    if is_command_execution_tool(tool_name) {
        let output = format_command_failure_output(&result);
        let preview = result
            .error
            .as_deref()
            .or_else(|| {
                if result.output.trim().is_empty() {
                    None
                } else {
                    Some(result.output.as_str())
                }
            })
            .unwrap_or("command failed");
        return NormalizedToolOutcome {
            output,
            success: false,
            error_reason: Some(preview.to_string()),
        };
    }

    let reason = result.error.unwrap_or(result.output);
    NormalizedToolOutcome {
        output: format!("Error: {reason}"),
        success: false,
        error_reason: Some(reason),
    }
}

fn format_command_failure_output(result: &ToolResult) -> String {
    let stderr = result.error.as_deref().unwrap_or("").trim();
    let stdout = result.output.trim();
    let is_timeout = stderr.contains("Command timed out after")
        || stdout.contains("Command timed out after");
    let mut parts = Vec::new();
    if is_timeout {
        parts.push(
            "[Command timed out and was killed. DO NOT retry the same command verbatim. \
             Either (a) pass a larger `timeout_ms`, or (b) set `background: true` and poll \
             via background_status / background_logs, or (c) split the work into smaller \
             steps, or (d) ask the user how to proceed. Repeating the identical command \
             will be refused by the loop guard.]"
                .to_string(),
        );
    } else {
        parts.push(
            "[Command finished with a non-zero exit code. Inspect stdout/stderr below; if \
             you have a concrete fix, apply it and try a DIFFERENT command. Do not invoke \
             the exact same command verbatim more than twice in a row \u{2014} the loop guard \
             will refuse identical retries. If the cause is unclear, report it to the user \
             via the ask tool instead of guessing.]"
                .to_string(),
        );
    }
    if !stdout.is_empty() {
        parts.push(format!("stdout:\n{stdout}"));
    }
    if !stderr.is_empty() {
        parts.push(format!("stderr:\n{stderr}"));
    }
    if stdout.is_empty() && stderr.is_empty() {
        parts.push("No output was captured.".to_string());
    }
    parts.join("\n\n")
}
