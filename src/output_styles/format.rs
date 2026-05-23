// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStyle {

    Default,

    Json,

    Markdown,

    Minimal,

    Verbose,

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
