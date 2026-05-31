// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::providers::traits::ChatMessage;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct ContextSignals {

    pub suggested_tools: Vec<String>,

    pub history_relevant: bool,
}

pub fn analyze_turn_context(
    history: &[ChatMessage],
    _user_message: &str,
    iteration: usize,
    last_tool_calls: &[String],
) -> ContextSignals {
    if iteration == 0 {
        return ContextSignals {
            suggested_tools: Vec::new(),
            history_relevant: true,
        };
    }

    let mut tools: HashSet<String> = HashSet::new();
    for tool in last_tool_calls {
        tools.insert(tool.clone());
    }

    if let Some(last_assistant) = history.iter().rev().find(|m| m.role == "assistant") {
        for word in last_assistant.content.split_whitespace() {
            for tool_name in tools_for_keyword(word) {
                tools.insert(String::from(*tool_name));
            }
        }
    }

    let mut suggested: Vec<String> = tools.into_iter().collect();
    suggested.sort();

    ContextSignals {
        suggested_tools: suggested,
        history_relevant: true,
    }
}

fn tools_for_keyword(keyword: &str) -> &'static [&'static str] {
    match keyword.to_lowercase().as_str() {
        "file" | "read" | "write" | "edit" | "path" | "directory" => {
            &["file_read", "file_write", "file_edit", "glob_search"]
        }
        "shell" | "command" | "run" | "execute" | "install" | "build" => &["shell"],
        "memory" | "remember" | "recall" | "store" | "forget" => &["memory_store", "memory_recall"],
        "search" | "find" | "grep" | "look" => {
            &["content_search", "glob_search", "web_search_tool"]
        }
        "browser" | "website" | "url" | "http" | "fetch" => &["web_fetch", "web_search_tool"],
        "image" | "screenshot" | "picture" => &["image_info"],
        "git" | "commit" | "branch" | "push" | "pull" => &["git_operations", "shell"],
        _ => &[],
    }
}
