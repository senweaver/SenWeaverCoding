// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// XML tag constants — mirrors claude-code-typescript-src`constants/xml.ts`.
// Standard XML tags used in system prompts and tool outputs.

pub const TAG_SYSTEM: (&str, &str) = ("<system>", "</system>");
pub const TAG_ENVIRONMENT: (&str, &str) = ("<environment>", "</environment>");
pub const TAG_TOOL_RESULT: (&str, &str) = ("<tool_result>", "</tool_result>");
pub const TAG_TOOL_ERROR: (&str, &str) = ("<tool_error>", "</tool_error>");
pub const TAG_FILE_CONTENT: (&str, &str) = ("<file_content>", "</file_content>");
pub const TAG_SEARCH_RESULTS: (&str, &str) = ("<search_results>", "</search_results>");
pub const TAG_COMMAND_OUTPUT: (&str, &str) = ("<command_output>", "</command_output>");
pub const TAG_AGENTS_MD: (&str, &str) = ("<agents_md>", "</agents_md>");
pub const TAG_MEMORY: (&str, &str) = ("<memory>", "</memory>");
pub const TAG_SESSION_MEMORIES: (&str, &str) = ("<session_memories>", "</session_memories>");
pub const TAG_PLAN: (&str, &str) = ("<plan>", "</plan>");
pub const TAG_THINKING: (&str, &str) = ("<thinking>", "</thinking>");
pub const TAG_SKILL: (&str, &str) = ("<skill>", "</skill>");
pub const TAG_CONTEXT: (&str, &str) = ("<context>", "</context>");
pub const TAG_NOTIFICATION: (&str, &str) = ("<notification>", "</notification>");

pub fn wrap(tag: (&str, &str), content: &str) -> String {
    format!("{}\n{}\n{}", tag.0, content, tag.1)
}

pub fn wrap_with_attr(tag_name: &str, attr: &str, content: &str) -> String {
    format!("<{tag_name} {attr}>\n{content}\n</{tag_name}>")
}
