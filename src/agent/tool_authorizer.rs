// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub fn canonical_tool_alias(name: &str) -> Option<&'static str> {
    let mapped = match name.to_ascii_lowercase().as_str() {
        "grep" | "ripgrep" | "rg" | "code_search" | "codesearch" | "search_files"
        | "searchfiles" | "search_code" => "content_search",
        "read" | "readfile" | "read_file" | "cat" | "view" | "viewfile" => "file_read",
        "write" | "writefile" | "create_file" | "createfile" => "file_write",
        "edit" | "str_replace" | "str_replace_editor" | "apply_patch" | "applypatch"
        | "edit_file" | "editfile" => "file_edit",
        "bash" | "sh" | "exec" | "command" | "cmd" | "terminal" | "run_command"
        | "runcommand" | "shell_command" => "shell",
        "web_search" | "websearch" | "web-search" | "search_web" | "websearch_tool" => {
            "web_search_tool"
        }
        "ls" | "list_files" | "listfiles" | "list_dir" | "listdir" | "dir" | "file_list"
        | "filelist" => "dir_list",
        "askquestion" => "ask_question",
        "askuser" => "ask_user",
        "memory_search" | "memorysearch" | "memrecall" | "memory_query" => "memory_recall",
        "lsp_symbols" | "lspsymbols" | "symbols" | "lsp_hover" | "lsphover"
        | "lsp_definition" => "lsp",
        _ => return None,
    };
    Some(mapped)
}

pub fn canonical_tool_name(name: &str) -> &str {
    canonical_tool_alias(name).unwrap_or(name)
}

pub fn authorize_tool_dispatch(tool_name: &str) -> Result<(), String> {
    if crate::security::estop::is_kill_all() {
        return Err(
            "Emergency stop engaged (kill_all): all tool execution is halted".to_string(),
        );
    }
    let canonical = canonical_tool_name(tool_name);
    if crate::security::estop::is_tool_frozen(tool_name)
        || (canonical != tool_name && crate::security::estop::is_tool_frozen(canonical))
    {
        return Err(format!(
            "Tool '{tool_name}' is frozen by an active emergency stop"
        ));
    }
    Ok(())
}
