// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;

use crate::providers::traits::{ConversationMessage, ToolResultMessage};

const SYNTHETIC_TOOL_REPLY: &str =
    "[Synthetic tool reply] No stored result exists for this tool_call_id in the transcript \
     (possible session interruption, context trim, or compaction). Ignore and continue.";

fn collect_followup_tool_call_ids(history: &[ConversationMessage], start: usize) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut j = start;
    while j < history.len() {
        match &history[j] {
            ConversationMessage::ToolResults(rows) => {
                for r in rows {
                    seen.insert(r.tool_call_id.clone());
                }
                j += 1;
            }
            ConversationMessage::Chat(c) if c.role == "tool" => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&c.content)
                    && let Some(id) = v.get("tool_call_id").and_then(serde_json::Value::as_str)
                {
                    seen.insert(id.to_string());
                }
                j += 1;
            }
            _ => break,
        }
    }
    seen
}

pub(crate) fn count_incomplete_followup_batches(messages: &[ConversationMessage]) -> (usize, usize) {
    let mut frames = 0usize;
    let mut stub_rows = 0usize;
    let mut i = 0usize;
    while i < messages.len() {
        if let ConversationMessage::AssistantToolCalls { tool_calls, .. } = &messages[i] {
            if !tool_calls.is_empty() {
                let required = tool_calls
                    .iter()
                    .filter_map(|c| {
                        let id = c.id.trim();
                        if id.is_empty() {
                            None
                        } else {
                            Some(c.id.clone())
                        }
                    })
                    .collect::<Vec<_>>();
                if !required.is_empty() {
                    let seen = collect_followup_tool_call_ids(messages, i + 1);
                    let missing_here = required.iter().filter(|id| !seen.contains(*id)).count();
                    if missing_here > 0 {
                        frames += 1;
                        stub_rows += missing_here;
                    }
                }
            }
        }
        i += 1;
    }
    (frames, stub_rows)
}

pub fn ensure_assistant_tool_replies_inplace(history: &mut Vec<ConversationMessage>) {
    let mut patches: usize = 0;
    let mut i = 0usize;
    while i < history.len() {
        if let ConversationMessage::AssistantToolCalls { tool_calls, .. } = &mut history[i] {
            for call in tool_calls.iter_mut() {
                if call.id.trim().is_empty() {
                    call.id = crate::providers::sanitize::normalize_tool_call_id_for_provider(
                        None,
                        crate::providers::sanitize::ProviderKind::Other,
                    );
                }
            }
        }

        let required: Option<Vec<String>> = match &history[i] {
            ConversationMessage::AssistantToolCalls { tool_calls, .. } if !tool_calls.is_empty() => {
                let ids = tool_calls
                    .iter()
                    .map(|c| c.id.clone())
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    None
                } else {
                    Some(ids)
                }
            }
            _ => None,
        };
        let Some(required) = required else {
            i += 1;
            continue;
        };

        let j = i + 1;
        let seen = collect_followup_tool_call_ids(history, j);
        let missing: Vec<String> = required
            .into_iter()
            .filter(|id| !seen.contains(id))
            .collect();
        if missing.is_empty() {
            i += 1;
            continue;
        }

        tracing::warn!(
            target: "agent.dangling_tool_repair",
            missing = ?missing,
            "injecting synthetic tool replies after incomplete assistant.tool_calls batch"
        );

        let stubs: Vec<ToolResultMessage> = missing
            .into_iter()
            .map(|tool_call_id| ToolResultMessage {
                tool_call_id,
                content: SYNTHETIC_TOOL_REPLY.to_string(),
            })
            .collect();

        patches += stubs.len();

        let mut k = i + 1;
        while k < history.len() {
            match &history[k] {
                ConversationMessage::ToolResults(_) => k += 1,
                ConversationMessage::Chat(c) if c.role == "tool" => k += 1,
                _ => break,
            }
        }
        history.insert(k, ConversationMessage::ToolResults(stubs));
        i += 1;
    }

    if patches > 0 {
        tracing::info!(
            target: "agent.dangling_tool_repair",
            stubs = patches,
            "applied synthetic tool reply rows for provider tool-call sequencing"
        );
    }
}

pub fn repair_dangling_tool_calls(mut messages: Vec<ConversationMessage>) -> Vec<ConversationMessage> {
    ensure_assistant_tool_replies_inplace(&mut messages);
    messages
}

pub fn has_dangling_tool_calls(messages: &[ConversationMessage]) -> bool {
    let mut i = 0usize;
    while i < messages.len() {
        if let ConversationMessage::AssistantToolCalls { tool_calls, .. } = &messages[i] {
            if !tool_calls.is_empty() {
                let required_ok = tool_calls.iter().any(|c| !c.id.trim().is_empty());
                if required_ok {
                    let required: Vec<String> = tool_calls
                        .iter()
                        .filter_map(|c| {
                            let id = c.id.trim();
                            if id.is_empty() {
                                None
                            } else {
                                Some(c.id.clone())
                            }
                        })
                        .collect();
                    let seen = collect_followup_tool_call_ids(messages, i + 1);
                    if required.iter().any(|id| !seen.contains(id)) {
                        return true;
                    }
                }
            }
        }
        i += 1;
    }
    false
}
