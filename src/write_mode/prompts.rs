// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::{MAX_PLAN_STEPS, types::PlanContext};

pub const PLAN_SYSTEM_PROMPT: &str = concat!(
    "You are a software-engineering planner.  Given a goal and a ",
    "workspace root, emit a JSON object of the form:\n",
    "\n",
    "{\n",
    "  \"goal\": \"<goal>\",\n",
    "  \"steps\": [\n",
    "    { \"kind\": \"read_file\",    \"path\": \"<rel path>\" },\n",
    "    { \"kind\": \"grep_symbol\",  \"query\": \"<name>\" },\n",
    "    { \"kind\": \"apply_diff\",   \"path\": \"<rel path>\", \"instruction\": \"<nl>\" },\n",
    "    { \"kind\": \"run_command\",  \"command\": \"<shell>\" },\n",
    "    { \"kind\": \"verify\",       \"expect_contains\": [\"<substr>\"] }\n",
    "  ]\n",
    "}\n",
    "\n",
    "Rules:\n",
    "  * 3-7 steps total.\n",
    "  * The last step MUST be of kind 'verify'.\n",
    "  * Use relative paths from the workspace root.\n",
    "  * Never reach outside the workspace.\n",
    "  * Do NOT include prose, markdown fences, or explanations — only ",
    "the JSON object.\n",
);

#[must_use]
pub fn build_plan_user_prompt(ctx: &PlanContext) -> String {
    let mut out = String::with_capacity(256 + ctx.goal.len());
    out.push_str("# Goal\n\n");
    out.push_str(&ctx.goal);
    out.push_str("\n\n# Workspace root\n\n");
    out.push_str(&ctx.workspace_root.display().to_string());
    out.push('\n');
    if let Some(h) = &ctx.hint {
        if !h.is_empty() {
            out.push_str("\n# Hint\n\n");
            out.push_str(h);
            out.push('\n');
        }
    }
    if !ctx.allow_paths.is_empty() {
        out.push_str("\n# Allowed paths (globs)\n\n");
        for g in &ctx.allow_paths {
            out.push_str("- ");
            out.push_str(g);
            out.push('\n');
        }
    }
    out.push_str(&format!(
        "\n# Limits\n\nReturn at most {MAX_PLAN_STEPS} steps.  Last step must be kind=verify.\n"
    ));
    out
}
