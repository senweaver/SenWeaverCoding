// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Dangling Tool Call Repair - fixes incomplete tool call histories.
//!
//! When an agent session is interrupted (crash, timeout, cancel), assistant
//! messages with tool_calls may lack corresponding tool results. This module
//! detects and patches those gaps so the LLM doesn't get confused by orphaned
//! calls.

use std::collections::HashSet;

use crate::providers::traits::{ConversationMessage, ToolResultMessage};

/// Scan messages for dangling tool calls and inject synthetic error results.
///
/// Returns the repaired message list. If no repairs needed, returns the input
/// unchanged.
pub fn repair_dangling_tool_calls(messages: Vec<ConversationMessage>) -> Vec<ConversationMessage> {
    let mut answered_ids: HashSet<String> = HashSet::new();
    for msg in &messages {
        if let ConversationMessage::ToolResults(results) = msg {
            for tr in results {
                answered_ids.insert(tr.tool_call_id.clone());
            }
        }
    }

    let mut repaired = Vec::with_capacity(messages.len() + 4);
    let mut patches_applied: usize = 0;

    for msg in messages {
        repaired.push(msg.clone());

        if let ConversationMessage::AssistantToolCalls { tool_calls, .. } = msg {
            let mut missing: Vec<ToolResultMessage> = Vec::new();
            for tc in tool_calls {
                if !answered_ids.contains(&tc.id) {
                    missing.push(ToolResultMessage {
                        tool_call_id: tc.id.clone(),
                        content: format!(
                            "[Interrupted] Tool '{}' was not completed due to a session \
                             interruption. The result is unavailable.",
                            tc.name
                        ),
                    });
                    answered_ids.insert(tc.id.clone());
                    patches_applied += 1;
                }
            }
            if !missing.is_empty() {
                repaired.push(ConversationMessage::ToolResults(missing));
            }
        }
    }

    if patches_applied > 0 {
        tracing::info!(
            patches = patches_applied,
            "Repaired dangling tool calls in conversation history"
        );
    }

    repaired
}

/// Check if any dangling tool calls exist without repairing them.
pub fn has_dangling_tool_calls(messages: &[ConversationMessage]) -> bool {
    let mut answered_ids: HashSet<String> = HashSet::new();
    for msg in messages {
        if let ConversationMessage::ToolResults(results) = msg {
            for tr in results {
                answered_ids.insert(tr.tool_call_id.clone());
            }
        }
    }

    for msg in messages {
        if let ConversationMessage::AssistantToolCalls { tool_calls, .. } = msg {
            for tc in tool_calls {
                if !answered_ids.contains(&tc.id) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::traits::{ChatMessage, ToolCall};

    fn make_tool_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }
    }

    #[test]
    fn test_no_dangling() {
        let messages = vec![
            ConversationMessage::Chat(ChatMessage::user("hello")),
            ConversationMessage::Chat(ChatMessage::assistant("hi")),
        ];
        assert!(!has_dangling_tool_calls(&messages));
        let repaired = repair_dangling_tool_calls(messages.clone());
        assert_eq!(repaired.len(), messages.len());
    }

    #[test]
    fn test_dangling_detected_and_repaired() {
        let messages = vec![
            ConversationMessage::Chat(ChatMessage::user("find something")),
            ConversationMessage::AssistantToolCalls {
                text: Some("I'll search for that".to_string()),
                tool_calls: vec![make_tool_call("tc-1", "web_search")],
                reasoning_content: None,
            },
        ];

        assert!(has_dangling_tool_calls(&messages));

        let repaired = repair_dangling_tool_calls(messages);
        assert_eq!(repaired.len(), 3);
        assert!(!has_dangling_tool_calls(&repaired));

        if let ConversationMessage::ToolResults(ref results) = repaired[2] {
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].tool_call_id, "tc-1");
            assert!(results[0].content.contains("Interrupted"));
        } else {
            panic!("Expected ToolResults as the third message");
        }
    }

    #[test]
    fn test_complete_calls_not_touched() {
        let messages = vec![
            ConversationMessage::Chat(ChatMessage::user("search")),
            ConversationMessage::AssistantToolCalls {
                text: Some("searching".to_string()),
                tool_calls: vec![make_tool_call("tc-1", "web_search")],
                reasoning_content: None,
            },
            ConversationMessage::ToolResults(vec![ToolResultMessage {
                tool_call_id: "tc-1".to_string(),
                content: "Found results".to_string(),
            }]),
        ];

        assert!(!has_dangling_tool_calls(&messages));
        let repaired = repair_dangling_tool_calls(messages.clone());
        assert_eq!(repaired.len(), 3);
    }

    #[test]
    fn test_multiple_dangling_calls() {
        let messages = vec![ConversationMessage::AssistantToolCalls {
            text: None,
            tool_calls: vec![
                make_tool_call("tc-1", "shell"),
                make_tool_call("tc-2", "file_read"),
            ],
            reasoning_content: None,
        }];

        assert!(has_dangling_tool_calls(&messages));

        let repaired = repair_dangling_tool_calls(messages);
        assert_eq!(repaired.len(), 2);

        if let ConversationMessage::ToolResults(ref results) = repaired[1] {
            assert_eq!(results.len(), 2);
        } else {
            panic!("Expected ToolResults");
        }
    }
}
