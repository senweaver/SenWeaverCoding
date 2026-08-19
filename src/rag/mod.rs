// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod embedding;
pub mod merkle_manifest;

pub mod vector_code_index;

use crate::memory::chunker;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DatasheetChunk {

    pub board: Option<String>,

    pub source: String,

    pub content: String,
}

pub type PinAliases = HashMap<String, u32>;

fn line_starts_with_ignore_case(line: &str, prefix: &str) -> bool {
    line.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn parse_pin_aliases(content: &str) -> PinAliases {
    let mut aliases = PinAliases::new();

    let section_markers = ["## pin aliases", "## pin alias", "## pins"];
    let mut in_section = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();

        if line_starts_with_ignore_case(line, "## ") {
            if section_markers
                .iter()
                .any(|marker| line_starts_with_ignore_case(line, marker))
            {
                in_section = true;
            } else if in_section {
                break;
            }
            continue;
        }

        if !in_section || line.is_empty() {
            continue;
        }

        if line.starts_with('|') {
            let parts: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
            if parts.len() >= 3 {
                let alias = parts[1].trim().to_lowercase().replace(' ', "_");
                let pin_str = parts[2].trim();

                if alias.eq("alias")
                    || alias.eq("pin")
                    || pin_str.eq("pin")
                    || alias.contains("---")
                    || pin_str.contains("---")
                {
                    continue;
                }
                if let Ok(pin) = pin_str.parse::<u32>() {
                    if !alias.is_empty() {
                        aliases.insert(alias, pin);
                    }
                }
            }
            continue;
        }

        if let Some((k, v)) = line.split_once(':').or_else(|| line.split_once('=')) {
            let alias = k.trim().to_lowercase().replace(' ', "_");
            if let Ok(pin) = v.trim().parse::<u32>() {
                if !alias.is_empty() {
                    aliases.insert(alias, pin);
                }
            }
        }
    }

    aliases
}

fn collect_md_txt_paths(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_txt_paths(&path, out);
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str());
            if ext == Some("md") || ext == Some("txt") {
                out.push(path);
            }
        }
    }
}

#[cfg(feature = "rag-pdf")]
fn collect_pdf_paths(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_pdf_paths(&path, out);
        } else if path.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
                out.push(path);
            }
        }
    }
}

#[cfg(feature = "rag-pdf")]
fn extract_pdf_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    pdf_extract::extract_text_from_mem(&bytes).ok()
}

pub struct HardwareRag {
    chunks: Vec<DatasheetChunk>,

    pin_aliases: HashMap<String, PinAliases>,
}

impl HardwareRag {

    pub fn load(workspace_dir: &Path, datasheet_dir: &str) -> anyhow::Result<Self> {
        let base = workspace_dir.join(datasheet_dir);
        if !base.exists() || !base.is_dir() {
            return Ok(Self {
                chunks: Vec::new(),
                pin_aliases: HashMap::new(),
            });
        }

        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        collect_md_txt_paths(&base, &mut paths);
        #[cfg(feature = "rag-pdf")]
        collect_pdf_paths(&base, &mut paths);

        let mut chunks = Vec::new();
        let mut pin_aliases: HashMap<String, PinAliases> = HashMap::new();
        let max_tokens = 512;

        for path in paths {
            let content = if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
                #[cfg(feature = "rag-pdf")]
                {
                    extract_pdf_text(&path).unwrap_or_default()
                }
                #[cfg(not(feature = "rag-pdf"))]
                {
                    String::new()
                }
            } else {
                std::fs::read_to_string(&path).unwrap_or_default()
            };

            if content.trim().is_empty() {
                continue;
            }

            let board = infer_board_from_path(&path, &base);
            let source = path
                .strip_prefix(workspace_dir)
                .unwrap_or(&path)
                .display()
                .to_string();

            let aliases = parse_pin_aliases(&content);
            if let Some(ref b) = board {
                if !aliases.is_empty() {
                    pin_aliases.insert(b.clone(), aliases);
                }
            }

            for chunk in chunker::chunk_markdown(&content, max_tokens) {
                chunks.push(DatasheetChunk {
                    board: board.clone(),
                    source: source.clone(),
                    content: chunk.content,
                });
            }
        }

        Ok(Self {
            chunks,
            pin_aliases,
        })
    }

    pub fn pin_aliases_for_board(&self, board: &str) -> Option<&PinAliases> {
        self.pin_aliases.get(board)
    }

    pub fn pin_alias_context(&self, query: &str, boards: &[String]) -> String {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower
            .split_whitespace()
            .filter(|w| w.len() > 1)
            .collect();

        let mut lines = Vec::new();
        for board in boards {
            if let Some(aliases) = self.pin_aliases.get(board) {
                for (alias, pin) in aliases {
                    let alias_words: Vec<&str> = alias.split('_').collect();
                    let matches = query_words.iter().any(|qw| alias_words.contains(qw))
                        || query_lower.contains(&alias.replace('_', " "));
                    if matches {
                        lines.push(format!("{board}: {alias} = pin {pin}"));
                    }
                }
            }
        }
        if lines.is_empty() {
            return String::new();
        }
        format!("[Pin aliases for query]\n{}\n\n", lines.join("\n"))
    }

    pub fn retrieve(&self, query: &str, boards: &[String], limit: usize) -> Vec<&DatasheetChunk> {
        if self.chunks.is_empty() || limit == 0 {
            return Vec::new();
        }

        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .collect();

        let mut scored: Vec<(&DatasheetChunk, f32)> = Vec::new();
        for chunk in &self.chunks {
            let content_lower = chunk.content.to_lowercase();
            let mut score = 0.0f32;

            for term in &query_terms {
                if content_lower.contains(term) {
                    score += 1.0;
                }
            }

            if score > 0.0 {
                let board_match = chunk.board.as_ref().map_or(false, |b| boards.contains(b));
                if board_match {
                    score += 2.0;
                }
                scored.push((chunk, score));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored.into_iter().map(|(c, _)| c).collect()
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

fn infer_board_from_path(path: &Path, base: &Path) -> Option<String> {
    let rel = path.strip_prefix(base).ok()?;
    let stem = path.file_stem()?.to_str()?;

    if stem == "generic" || stem.starts_with("generic_") {
        return None;
    }
    if rel.parent().and_then(|p| p.to_str()) == Some("_generic") {
        return None;
    }

    Some(stem.to_string())
}
