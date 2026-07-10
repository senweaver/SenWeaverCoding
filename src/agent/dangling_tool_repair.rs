// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashSet;

use crate::providers::traits::{ConversationMessage, ToolResultMessage};

const SYNTHETIC_TOOL_REPLY: &str =
    "[Synthetic tool reply] No stored result exists for this tool_call_id in the transcript \
     (possible session interruption, context trim, or compaction). This is not an error; use \
     the conversation context and the user's latest message to decide how to proceed.";

const INTERRUPTED_TURN_NOTE: &str =
    "[System note: your MOST RECENT task - the user request immediately above this note - was \
     interrupted (cancelled, errored, or the app restarted) before it finished. This note refers \
     ONLY to that single most-recent task. Resume it ONLY when the user's latest message \
     EXPLICITLY asks to continue or finish it (e.g. \"继续\" / \"continue\" / \"接着\" / \"go on\") \
     or directly references that task; in that case pick up from where it stopped and do NOT redo \
     already-completed steps. If the latest message is anything else - a greeting (\"你好\" / \
     \"hi\" / \"在吗\"), small talk, an acknowledgement, or a new or unrelated request - treat IT \
     as the authoritative current request, answer it directly, and do NOT resume or re-run the \
     interrupted task on your own. Never treat a short or ambiguous message as implicit consent to \
     keep going. Never resume, restate, or merge in any OLDER interrupted, stopped, superseded, or \
     already-finished task from earlier in this conversation (including earlier design-task \
     contracts and their artifacts) - those are done or abandoned. Judge strictly from the user's \
     literal latest message.]";

pub fn drop_payloadless_assistant_messages(history: &mut Vec<ConversationMessage>) {
    let before = history.len();
    history.retain(|msg| !is_payloadless_assistant(msg));
    let dropped = before - history.len();
    if dropped > 0 {
        tracing::warn!(
            target: "agent.dangling_tool_repair",
            dropped,
            "dropped payload-less assistant turns (no content, no tool_calls) from history so they are never seeded or sent to the model"
        );
    }
}

fn is_payloadless_assistant(msg: &ConversationMessage) -> bool {
    match msg {
        ConversationMessage::Chat(c) => {
            c.role == "assistant" && crate::providers::sanitize::assistant_has_no_payload(&c.content)
        }
        ConversationMessage::AssistantToolCalls {
            text,
            tool_calls,
            reasoning_content,
        } => {
            tool_calls.is_empty()
                && text.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true)
                && reasoning_content
                    .as_deref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
        }
        ConversationMessage::ToolResults(_) => false,
    }
}

pub fn is_interrupted_turn_note(content: &str) -> bool {
    content == INTERRUPTED_TURN_NOTE
}

pub fn is_orphan_close_note(content: &str) -> bool {
    content == ORPHAN_CLOSE_NOTE
}

pub fn is_turn_close_note(content: &str) -> bool {
    is_interrupted_turn_note(content) || is_orphan_close_note(content)
}

pub fn tail_signals_interrupted_turn(history: &[ConversationMessage]) -> bool {
    match history.last() {
        Some(ConversationMessage::Chat(c)) if c.role == "user" => true,
        Some(ConversationMessage::ToolResults(rows)) => {
            rows.iter().any(|r| r.content == SYNTHETIC_TOOL_REPLY)
        }
        _ => false,
    }
}

pub fn note_interrupted_turn(history: &mut Vec<ConversationMessage>) {
    let already_noted = matches!(
        history.last(),
        Some(ConversationMessage::Chat(c))
            if c.role == "assistant" && c.content == INTERRUPTED_TURN_NOTE
    );
    if already_noted {
        return;
    }
    tracing::info!(
        target: "agent.dangling_tool_repair",
        "marking the interrupted prior turn as abandoned before the new (fresh) user request"
    );
    history.push(ConversationMessage::Chat(
        crate::providers::traits::ChatMessage::assistant(INTERRUPTED_TURN_NOTE),
    ));
}

const ORPHAN_CLOSE_NOTE: &str =
    "[System note: the previous turn ended without an assistant reply (interrupted, cancelled, or \
     the app restarted). Treat the latest [CURRENT REQUEST] below as the authoritative instruction \
     for what to do now. Do not infer a task to resume from this note.]";

pub fn close_orphan_user_turns(
    history: &mut Vec<ConversationMessage>,
    has_authoritative_unfinished: bool,
) {
    let ends_with_user = matches!(
        history.last(),
        Some(ConversationMessage::Chat(c)) if c.role == "user"
    );
    if ends_with_user {
        let note = if has_authoritative_unfinished {
            ORPHAN_CLOSE_NOTE
        } else {
            INTERRUPTED_TURN_NOTE
        };
        tracing::warn!(
            target: "agent.dangling_tool_repair",
            neutral = has_authoritative_unfinished,
            "closing orphan user turn from an interrupted/crashed prior turn before new request"
        );
        history.push(ConversationMessage::Chat(
            crate::providers::traits::ChatMessage::assistant(note),
        ));
    }
}

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

        tracing::debug!(
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
    drop_payloadless_assistant_messages(&mut messages);
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
