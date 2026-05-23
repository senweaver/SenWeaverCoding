// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSection {
    pub title: String,
    pub content: String,
    pub source_files: Vec<String>,
    pub doc_type: DocType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    Readme,
    ApiReference,
    SetupGuide,
    Architecture,
    Changelog,
    Contributing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MagicDocsConfig {
    pub include_examples: bool,
    pub max_depth: u32,
    pub include_private: bool,
    pub output_format: DocFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocFormat {
    Markdown,
    Html,
    PlainText,
}

impl Default for MagicDocsConfig {
    fn default() -> Self {
        Self {
            include_examples: true,
            max_depth: 3,
            include_private: false,
            output_format: DocFormat::Markdown,
        }
    }
}

pub fn generate_structure_doc(
    project_name: &str,
    directories: &[DirectoryInfo],
    _config: &MagicDocsConfig,
) -> DocSection {
    let mut content = format!("# {project_name}\n\n## Project Structure\n\n");
    for dir in directories {
        content.push_str(&format!(
            "- **`{}/`** — {} ({} files)\n",
            dir.path, dir.description, dir.file_count
        ));
    }
    DocSection {
        title: format!("{project_name} — Project Structure"),
        content,
        source_files: directories.iter().map(|d| d.path.clone()).collect(),
        doc_type: DocType::Architecture,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryInfo {
    pub path: String,
    pub description: String,
    pub file_count: u32,
}
