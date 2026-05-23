// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct MemoryFileContext {

    pub agents_md: Vec<MemoryFile>,

    pub claude_md: Vec<MemoryFile>,

    pub memory_files: Vec<MemoryFile>,
}

#[derive(Debug, Clone)]
pub struct MemoryFile {
    pub path: PathBuf,
    pub content: String,
    pub source: MemoryFileSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFileSource {
    ProjectRoot,
    UserHome,
    AdditionalDir,
}

impl MemoryFileContext {

    pub async fn load(search_dirs: &[PathBuf]) -> Self {
        let mut ctx = Self::default();

        for (idx, dir) in search_dirs.iter().enumerate() {
            let source = if idx == 0 {
                MemoryFileSource::ProjectRoot
            } else {
                MemoryFileSource::AdditionalDir
            };

            let agents_path = dir.join("AGENTS.md");
            if let Ok(content) = tokio::fs::read_to_string(&agents_path).await {
                ctx.agents_md.push(MemoryFile {
                    path: agents_path,
                    content,
                    source,
                });
            }

            let claude_path = dir.join("CLAUDE.md");
            if let Ok(content) = tokio::fs::read_to_string(&claude_path).await {
                ctx.claude_md.push(MemoryFile {
                    path: claude_path,
                    content,
                    source,
                });
            }

            let memory_dir = dir.join(".senweavercoding").join("memory");
            if memory_dir.is_dir() {
                if let Ok(mut entries) = tokio::fs::read_dir(&memory_dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("md") {
                            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                                ctx.memory_files.push(MemoryFile {
                                    path,
                                    content,
                                    source,
                                });
                            }
                        }
                    }
                }
            }
        }

        if let Some(home) = home_dir() {
            let home_agents = home.join(".senweavercoding").join("AGENTS.md");
            if let Ok(content) = tokio::fs::read_to_string(&home_agents).await {
                ctx.agents_md.push(MemoryFile {
                    path: home_agents,
                    content,
                    source: MemoryFileSource::UserHome,
                });
            }
        }

        ctx
    }

    pub fn build_prompt(&self, max_chars: usize) -> String {
        let mut parts = Vec::new();
        let mut total_len = 0;

        for file in &self.agents_md {
            if total_len + file.content.len() > max_chars {
                break;
            }
            parts.push(format!(
                "<agents_md path=\"{}\">\n{}\n</agents_md>",
                file.path.display(),
                file.content
            ));
            total_len += file.content.len();
        }

        for file in &self.claude_md {
            if total_len + file.content.len() > max_chars {
                break;
            }
            parts.push(format!(
                "<claude_md path=\"{}\">\n{}\n</claude_md>",
                file.path.display(),
                file.content
            ));
            total_len += file.content.len();
        }

        for file in &self.memory_files {
            if total_len + file.content.len() > max_chars {
                break;
            }
            parts.push(format!(
                "<memory_file path=\"{}\">\n{}\n</memory_file>",
                file.path.display(),
                file.content
            ));
            total_len += file.content.len();
        }

        parts.join("\n\n")
    }

    pub fn is_empty(&self) -> bool {
        self.agents_md.is_empty() && self.claude_md.is_empty() && self.memory_files.is_empty()
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}
