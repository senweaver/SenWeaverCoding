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
            "- **`{}/`**  -  {} ({} files)\n",
            dir.path, dir.description, dir.file_count
        ));
    }
    DocSection {
        title: format!("{project_name}  -  Project Structure"),
        content,
        source_files: directories.iter().map(|d| d.path.clone()).collect(),
        doc_type: DocType::Architecture,
    }
}

pub fn scan_workspace_directories(root: &std::path::Path, max_depth: u32) -> Vec<DirectoryInfo> {
    let mut dirs = Vec::new();
    if !root.is_dir() {
        return dirs;
    }
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    collect_workspace_directories(root, &root_name, max_depth, &mut dirs);
    dirs
}

fn collect_workspace_directories(
    dir: &std::path::Path,
    rel: &str,
    depth: u32,
    out: &mut Vec<DirectoryInfo>,
) {
    if depth == 0 {
        return;
    }
    let mut file_count = 0u32;
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_file() {
            file_count = file_count.saturating_add(1);
        } else if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            collect_workspace_directories(&path, &child_rel, depth.saturating_sub(1), out);
        }
    }
    out.push(DirectoryInfo {
        path: rel.to_string(),
        description: describe_directory(rel),
        file_count,
    });
}

fn describe_directory(rel: &str) -> String {
    match rel.rsplit('/').next().unwrap_or(rel) {
        "src" => "source code".into(),
        "docs" => "documentation".into(),
        "tests" | "test" => "tests".into(),
        "benches" => "benchmarks".into(),
        "examples" => "examples".into(),
        "desktop" => "desktop application".into(),
        "sdk" => "SDK packages".into(),
        _ => "project directory".into(),
    }
}

pub fn structure_doc_for_workspace(
    root: &std::path::Path,
    config: &MagicDocsConfig,
) -> DocSection {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    let directories = scan_workspace_directories(root, config.max_depth);
    generate_structure_doc(&name, &directories, config)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryInfo {
    pub path: String,
    pub description: String,
    pub file_count: u32,
}
