// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Prompt constants — mirrors claude-code-typescript-src`constants/prompts.ts`.

/// Default compaction summary prompt.
pub const COMPACT_SUMMARY_PROMPT: &str = "\
Summarize the conversation so far in a concise way that preserves all important \
context, decisions made, file paths mentioned, code changes performed, and any \
pending tasks. Focus on information the assistant will need to continue helping \
effectively.";

/// Plan mode instruction prompt.
pub const PLAN_MODE_INSTRUCTION: &str = "\
You are in PLAN mode. Do NOT make any code changes or execute tools that modify \
the filesystem. Instead, analyze the request and produce a detailed plan. \
Outline the approach, list files to modify, and describe each change. \
Wait for user approval before switching to implementation.";

/// Auto-mode instruction prompt.
pub const AUTO_MODE_INSTRUCTION: &str = "\
You are in AUTO mode. You may execute tools without asking for explicit approval \
for each step. Proceed autonomously but stay focused on the user's request. \
If you encounter ambiguity or a decision with significant consequences, pause \
and ask the user.";

/// Coordinator mode instruction prompt.
pub const COORDINATOR_MODE_INSTRUCTION: &str = "\
You are in COORDINATOR mode. You manage multiple sub-agents (teammates). \
Delegate tasks by creating sub-agent tasks and monitor their progress. \
Synthesize results and present a unified response to the user.";

/// Tool error recovery prompt.
pub const TOOL_ERROR_RECOVERY: &str = "\
The previous tool call returned an error. Analyze the error, determine the \
root cause, and either retry with corrected parameters or explain the issue \
to the user.";

/// Context approaching limit warning.
pub const CONTEXT_LIMIT_WARNING: &str = "\
NOTE: The conversation context is approaching its limit. Consider using /compact \
to summarize the conversation and free up space, or start a new conversation \
for a fresh context.";

/// Default greeting for interactive mode.
pub const INTERACTIVE_GREETING: &str = "\
What would you like to work on? I can help with coding tasks, file operations, \
debugging, and more. Type /help for available commands.";
