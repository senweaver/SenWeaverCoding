// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct AskQuestionTool;

impl AskQuestionTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AskQuestionTool {
    fn name(&self) -> &str {
        "ask_question"
    }

    fn description(&self) -> &str {
        "Ask the user a clarifying question with selectable options. \
         Use this before creating a plan when the task is ambiguous or \
         has multiple valid approaches. Present 2-6 clear options. \
         Set `allow_multiple: true` for select-all-that-apply questions \
         where the user may legitimately pick more than one option \
         (e.g. \"which subsystems should this touch?\"); leave it false \
         for either/or decisions. The user's selections are returned as \
         the tool result; multi-select answers come back as a list of \
         labels."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["questions"],
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "Array of questions to ask the user",
                    "items": {
                        "type": "object",
                        "required": ["id", "prompt", "options"],
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique identifier for this question"
                            },
                            "prompt": {
                                "type": "string",
                                "description": "The question text to display"
                            },
                            "options": {
                                "type": "array",
                                "description": "Selectable options (2-6 items)",
                                "items": {
                                    "type": "object",
                                    "required": ["id", "label"],
                                    "properties": {
                                        "id": {
                                            "type": "string",
                                            "description": "Unique option identifier"
                                        },
                                        "label": {
                                            "type": "string",
                                            "description": "Display text for this option"
                                        }
                                    }
                                }
                            },
                            "allow_multiple": {
                                "type": "boolean",
                                "description": "Set to true when the user may pick more than one option (select-all-that-apply). Defaults to false (single-choice).",
                                "default": false
                            }
                        }
                    }
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {

        let questions = args
            .get("questions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if questions.is_empty() {
            return Ok(ToolResult {
                output: "No questions provided.".to_string(),
                success: false,
                error: Some("No questions in request".to_string()),
            });
        }

        let skipped = args
            .get("skipped")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let answers_obj = args.get("answers").and_then(|v| v.as_object()).cloned();
        let details = args
            .get("details")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let has_user_response = answers_obj
            .as_ref()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
            || details.is_some()
            || skipped;

        if !has_user_response {
            return Ok(ToolResult {
                output: "__WAITING_FOR_USER_RESPONSE__".to_string(),
                success: true,
                error: None,
            });
        }

        let mut buf = String::new();
        buf.push_str("User answered the clarifying question(s):\n\n");
        for (idx, q) in questions.iter().enumerate() {
            let qid = q
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("q-{idx}"));
            let prompt = q
                .get("prompt")
                .or_else(|| q.get("question"))
                .and_then(|v| v.as_str())
                .unwrap_or("(no prompt)");
            let allow_multiple = q
                .get("allow_multiple")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let raw_answer = answers_obj
                .as_ref()
                .and_then(|a| a.get(&qid).or_else(|| a.get(prompt)));
            let labels: Vec<String> = match raw_answer {
                Some(v) if v.is_string() => v
                    .as_str()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| vec![s.to_string()])
                    .unwrap_or_default(),
                Some(serde_json::Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|item| match item {
                        serde_json::Value::String(s) => {
                            let t = s.trim();
                            if t.is_empty() {
                                None
                            } else {
                                Some(t.to_string())
                            }
                        }
                        serde_json::Value::Object(map) => map
                            .get("label")
                            .or_else(|| map.get("text"))
                            .or_else(|| map.get("id"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty()),
                        other => {
                            let s = other.to_string();
                            if s.is_empty() { None } else { Some(s) }
                        }
                    })
                    .collect(),
                Some(other) => {
                    let s = other.to_string();
                    if s.is_empty() {
                        Vec::new()
                    } else {
                        vec![s]
                    }
                }
                None => Vec::new(),
            };
            if labels.is_empty() {
                let placeholder = if skipped { "(skipped)" } else { "(no answer)" };
                buf.push_str(&format!("{}. {prompt}\n   ↳ {placeholder}\n", idx + 1));
            } else if allow_multiple || labels.len() > 1 {
                buf.push_str(&format!("{}. {prompt}\n", idx + 1));
                for label in &labels {
                    buf.push_str(&format!("   ↳ {label}\n"));
                }
            } else {
                buf.push_str(&format!("{}. {prompt}\n   ↳ {}\n", idx + 1, labels[0]));
            }
        }
        if let Some(d) = details {
            buf.push_str(&format!("\nAdditional details from the user:\n{d}\n"));
        }
        if skipped && answers_obj.as_ref().map(|a| a.is_empty()).unwrap_or(true) {
            buf.push_str(
                "\nThe user skipped the question batch — proceed with reasonable defaults \
                 and note any assumptions in the plan.\n",
            );
        } else {
            buf.push_str(
                "\nProceed with planning using the answers above.  Do NOT ask the same \
                 questions again unless the user's reply is genuinely ambiguous.\n",
            );
        }

        Ok(ToolResult {
            output: buf,
            success: true,
            error: None,
        })
    }
}
