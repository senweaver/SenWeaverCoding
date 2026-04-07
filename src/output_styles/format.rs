// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! CLI / tool result output formatting (distinct from loaded prompt styles in [`super::types`]).
//!
//! Use [`OutputStyle`] for how tool results and agent responses are formatted on the wire or console.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStyle {
    /// Default text output
    Default,
    /// JSON output
    Json,
    /// Markdown formatted output
    Markdown,
    /// Minimal output (no decorations)
    Minimal,
    /// Verbose output with timing and metadata
    Verbose,
    /// Stream JSON (line-delimited JSON)
    StreamJson,
}

impl Default for OutputStyle {
    fn default() -> Self {
        Self::Default
    }
}

impl std::fmt::Display for OutputStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Json => write!(f, "json"),
            Self::Markdown => write!(f, "markdown"),
            Self::Minimal => write!(f, "minimal"),
            Self::Verbose => write!(f, "verbose"),
            Self::StreamJson => write!(f, "stream-json"),
        }
    }
}

impl std::str::FromStr for OutputStyle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "json" => Ok(Self::Json),
            "markdown" | "md" => Ok(Self::Markdown),
            "minimal" | "min" => Ok(Self::Minimal),
            "verbose" => Ok(Self::Verbose),
            "stream-json" | "stream_json" | "sjson" => Ok(Self::StreamJson),
            _ => Err(format!(
                "Unknown output style: '{}'. Valid: default, json, markdown, minimal, verbose, stream-json",
                s
            )),
        }
    }
}

/// Format a tool result for display based on the output style.
pub fn format_tool_result(
    tool_name: &str,
    output: &str,
    success: bool,
    style: OutputStyle,
) -> String {
    match style {
        OutputStyle::Json => serde_json::json!({
            "tool": tool_name,
            "success": success,
            "output": output,
        })
        .to_string(),
        OutputStyle::Minimal => output.to_string(),
        OutputStyle::Verbose => {
            format!(
                "[{}] {} ({})\n{}",
                if success { "OK" } else { "ERR" },
                tool_name,
                chrono::Utc::now().to_rfc3339(),
                output
            )
        }
        _ => {
            if success {
                format!("{}: {}", tool_name, output)
            } else {
                format!("{} [error]: {}", tool_name, output)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_output_style() {
        assert_eq!("json".parse::<OutputStyle>().unwrap(), OutputStyle::Json);
        assert_eq!(
            "markdown".parse::<OutputStyle>().unwrap(),
            OutputStyle::Markdown
        );
        assert_eq!("md".parse::<OutputStyle>().unwrap(), OutputStyle::Markdown);
        assert!("invalid".parse::<OutputStyle>().is_err());
    }

    #[test]
    fn display_output_style() {
        assert_eq!(OutputStyle::Json.to_string(), "json");
        assert_eq!(OutputStyle::Default.to_string(), "default");
    }

    #[test]
    fn format_json_style() {
        let result = format_tool_result("test", "hello", true, OutputStyle::Json);
        assert!(result.contains("\"success\":true"));
    }

    #[test]
    fn format_minimal_style() {
        let result = format_tool_result("test", "hello", true, OutputStyle::Minimal);
        assert_eq!(result, "hello");
    }
}
