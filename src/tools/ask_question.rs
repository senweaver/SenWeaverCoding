// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Tool for asking the user clarifying questions with selectable options.
//! Used in Plan mode before generating a plan to narrow down requirements.

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
         The user's selections are returned as the tool result."
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
                                "description": "Whether multiple options can be selected. Defaults to false.",
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
            let raw_answer = answers_obj
                .as_ref()
                .and_then(|a| a.get(&qid).or_else(|| a.get(prompt)));
            let answer_label = match raw_answer {
                Some(v) if v.is_string() => v.as_str().unwrap_or("").to_string(),
                Some(v) => v.to_string(),
                None => {
                    if skipped {
                        "(skipped)".to_string()
                    } else {
                        "(no answer)".to_string()
                    }
                }
            };
            buf.push_str(&format!(
                "{}. {prompt}\n   ↳ {answer_label}\n",
                idx + 1
            ));
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
