// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Prompt templates for inline-edit LLM requests.

use super::request::InlineEditRequest;

pub const SYSTEM_PROMPT: &str = "\
You are an expert code refactoring assistant.  When given a code \
selection and an instruction, you respond ONLY with a unified diff \
(hunks prefixed with @@) relative to the selection.  Do not add \
commentary.  Do not include the selection outside of a hunk context \
line.  Every hunk must be minimal.
";

pub fn build_instruction_prompt(req: &InlineEditRequest) -> String {
    let mut out = String::new();
    out.push_str("Instruction:\n");
    out.push_str(&req.instruction);
    out.push_str("\n\nFile: ");
    out.push_str(&req.file_path.display().to_string());
    out.push_str("\n\nSelection (between <<< and >>>):\n<<<\n");
    out.push_str(&req.selection);
    if !req.selection.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(">>>\n");
    if let Some(ctx) = &req.context_lines
        && !ctx.is_empty()
    {
        out.push_str("\nSurrounding context:\n");
        for line in ctx {
            out.push_str(line);
            if !line.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out.push_str("\nRespond with a unified diff only.\n");
    out
}
