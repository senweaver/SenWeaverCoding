// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Memdir — filesystem-backed auto-memory around CLAUDE.md / AGENTS.md / MEMORY.md.
//!
//! Mirrors cc-typescript-src's `memdir/` module. Discovers and loads
//! memory instruction files from the workspace and its parent directories,
//! building a combined prompt for the agent.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Well-known memory file names (searched in order).
pub const MEMORY_FILE_NAMES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    "MEMORY.md",
    ".claude",
    ".cursorrules",
];

/// Maximum size of a single memory file to include (100 KB).
const MAX_MEMORY_FILE_SIZE: u64 = 100 * 1024;

/// Maximum total memory prompt size (500 KB).
const MAX_TOTAL_MEMORY_SIZE: usize = 500 * 1024;

/// A discovered memory file with its content.
#[derive(Debug, Clone)]
pub struct MemoryFile {
    pub path: PathBuf,
    pub name: String,
    pub content: String,
    pub is_project_root: bool,
}

/// Discover memory files by walking up from the given directory.
///
/// Searches the workspace directory and its parents for known memory files
/// (CLAUDE.md, AGENTS.md, MEMORY.md, .claude, .cursorrules).
/// Files are returned in order from most specific (deepest) to least specific (root).
pub async fn discover_memory_files(workspace: &Path) -> Result<Vec<MemoryFile>> {
    let mut files = Vec::new();
    let mut current = workspace.to_path_buf();
    let mut is_first = true;

    // Walk up from workspace to root, collecting memory files
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

/// Build a combined memory prompt from discovered files.
///
/// Concatenates all memory file contents with headers, respecting
/// the total size limit. Files are included in order from most
/// specific to least specific.
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

        let entry_size = header.len() + file.content.len() + 2; // +2 for newlines
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

/// Find relevant memories for a given query by scanning file names
/// and checking for keyword overlap.
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

/// Truncate memory content to a maximum number of lines.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_file_names_includes_expected() {
        assert!(MEMORY_FILE_NAMES.contains(&"CLAUDE.md"));
        assert!(MEMORY_FILE_NAMES.contains(&"AGENTS.md"));
        assert!(MEMORY_FILE_NAMES.contains(&"MEMORY.md"));
    }

    #[test]
    fn build_prompt_empty() {
        let prompt = build_memory_prompt(&[]);
        assert!(prompt.is_empty());
    }

    #[test]
    fn build_prompt_single_file() {
        let files = vec![MemoryFile {
            path: PathBuf::from("/project/CLAUDE.md"),
            name: "CLAUDE.md".into(),
            content: "Test instructions".into(),
            is_project_root: true,
        }];
        let prompt = build_memory_prompt(&files);
        assert!(prompt.contains("CLAUDE.md"));
        assert!(prompt.contains("Test instructions"));
        assert!(prompt.contains("[project root]"));
    }

    #[test]
    fn truncate_short_content() {
        let content = "line1\nline2\nline3";
        assert_eq!(truncate_content(content, 10), content);
    }

    #[test]
    fn truncate_long_content() {
        let content = "a\nb\nc\nd\ne\nf";
        let truncated = truncate_content(content, 3);
        assert!(truncated.contains("a\nb\nc"));
        assert!(truncated.contains("3 more lines truncated"));
    }

    #[test]
    fn find_relevant_matches_keywords() {
        let files = vec![
            MemoryFile {
                path: PathBuf::from("a.md"),
                name: "a.md".into(),
                content: "This is about authentication and security".into(),
                is_project_root: false,
            },
            MemoryFile {
                path: PathBuf::from("b.md"),
                name: "b.md".into(),
                content: "This is about database migrations".into(),
                is_project_root: false,
            },
        ];
        let relevant = find_relevant_memories(&files, "security auth");
        assert_eq!(relevant.len(), 1);
        assert_eq!(relevant[0].name, "a.md");
    }

    #[tokio::test]
    async fn discover_empty_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let files = discover_memory_files(tmp.path()).await.unwrap();
        // May find files from parent directories, but should not panic
        let project_files: Vec<_> = files.iter().filter(|f| f.is_project_root).collect();
        assert!(project_files.is_empty());
    }

    #[tokio::test]
    async fn discover_with_claude_md() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("CLAUDE.md"), "# Project Rules\nBe nice.")
            .await
            .unwrap();
        let files = discover_memory_files(tmp.path()).await.unwrap();
        let project_files: Vec<_> = files.iter().filter(|f| f.is_project_root).collect();
        assert!(!project_files.is_empty());
        assert!(project_files[0].content.contains("Be nice"));
    }
}
