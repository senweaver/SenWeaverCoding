// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Prompt constants — mirrors claude-code-typescript-src`constants/prompts.ts`.

pub const COMPACT_SUMMARY_PROMPT: &str = "\
Summarize the conversation so far in a concise way that preserves all important \
context, decisions made, file paths mentioned, code changes performed, and any \
pending tasks. Focus on information the assistant will need to continue helping \
effectively.";

pub const AUTO_MODE_INSTRUCTION: &str = "\
You are in AUTO mode. You may execute tools without asking for explicit approval \
for each step. Proceed autonomously but stay focused on the user's request. \
If you encounter ambiguity or a decision with significant consequences, pause \
and ask the user.";

pub const COORDINATOR_MODE_INSTRUCTION: &str = "\
You are in COORDINATOR mode. You manage multiple sub-agents (teammates). \
Delegate tasks by creating sub-agent tasks and monitor their progress. \
Synthesize results and present a unified response to the user.";

pub const TOOL_ERROR_RECOVERY: &str = "\
The previous tool call returned an error. Analyze the error, determine the \
root cause, and either retry with corrected parameters or explain the issue \
to the user.";

pub const CONTEXT_LIMIT_WARNING: &str = "\
NOTE: The conversation context is approaching its limit. Consider using /compact \
to summarize the conversation and free up space, or start a new conversation \
for a fresh context.";

pub const INTERACTIVE_GREETING: &str = "\
What would you like to work on? I can help with coding tasks, file operations, \
debugging, and more. Type /help for available commands.";
