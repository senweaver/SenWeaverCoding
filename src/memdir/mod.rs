// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub const MEMORY_FILE_NAMES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    "MEMORY.md",
    ".claude",
    ".cursorrules",
];

const MAX_MEMORY_FILE_SIZE: u64 = 100 * 1024;

const MAX_TOTAL_MEMORY_SIZE: usize = 500 * 1024;

#[derive(Debug, Clone)]
pub struct MemoryFile {
    pub path: PathBuf,
    pub name: String,
    pub content: String,
    pub is_project_root: bool,
}

pub async fn discover_memory_files(workspace: &Path) -> Result<Vec<MemoryFile>> {
    let mut files = Vec::new();
    let mut current = workspace.to_path_buf();
    let mut is_first = true;

    loop {
        for name in MEMORY_FILE_NAMES {
            let candidate = current.join(name);
            if candidate.is_file() {
                if let Ok(meta) = tokio::fs::metadata(&candidate).await {
                    if meta.len() <= MAX_MEMORY_FILE_SIZE {
                        if let Ok(content) = tokio::fs::read_to_string(&candidate).await {
                            files.push(MemoryFile {
                                path: candidate,
                                name: name.to_string(),
                                content,
                                is_project_root: is_first,
                            });
                        }
                    } else {
                        tracing::warn!(
                            "Memory file too large, skipping: {} ({} bytes)",
                            candidate.display(),
                            meta.len()
                        );
                    }
                }
            }
        }

        is_first = false;
        if !current.pop() {
            break;
        }
    }

    Ok(files)
}

pub fn build_memory_prompt(files: &[MemoryFile]) -> String {
    let mut prompt = String::new();
    let mut total_size = 0;

    for file in files {
        let header = format!(
            "\n--- Memory: {} ({}){} ---\n",
            file.name,
            file.path.display(),
            if file.is_project_root {
                " [project root]"
            } else {
                ""
            }
        );

        let entry_size = header.len() + file.content.len() + 2;
        if total_size + entry_size > MAX_TOTAL_MEMORY_SIZE {
            tracing::warn!("Memory prompt size limit reached, skipping remaining files");
            break;
        }

        prompt.push_str(&header);
        prompt.push_str(&file.content);
        prompt.push('\n');
        total_size += entry_size;
    }

    prompt
}

pub fn find_relevant_memories<'a>(files: &'a [MemoryFile], query: &str) -> Vec<&'a MemoryFile> {
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();

    files
        .iter()
        .filter(|f| {
            let content_lower = f.content.to_lowercase();
            query_words
                .iter()
                .any(|word| word.len() > 3 && content_lower.contains(word))
        })
        .collect()
}

pub fn truncate_content(content: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        content.to_string()
    } else {
        let truncated: Vec<&str> = lines[..max_lines].to_vec();
        format!(
            "{}\n... ({} more lines truncated)",
            truncated.join("\n"),
            lines.len() - max_lines
        )
    }
}
