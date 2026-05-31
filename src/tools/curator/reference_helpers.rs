// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context as _, Result};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static REF_ID_BY_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([GL])(\d+)\]").expect("ref id regex compile"));

pub const REFS_GIT_SUBDIR: &str = "refs/git";

pub const REFERENCE_HEAD_DOC_CHARS: usize = 4_000;
pub const REFERENCE_SKELETON_HEAD_CHARS: usize = 1_400;

#[derive(Debug, Clone, Default)]
pub struct RepoMetadata {
    pub readme_path: Option<String>,
    pub readme_excerpt: Option<String>,
    pub license_name: Option<String>,
    pub license_path: Option<String>,
    pub agents_md: Option<String>,
    pub contributing: Option<String>,
    pub architecture_doc: Option<String>,
    pub build_manifests: Vec<BuildManifest>,
    pub doc_dirs: Vec<String>,
    pub primary_languages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BuildManifest {
    pub name: String,
    pub path: String,
    pub head_excerpt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodeFile {
    pub relative_path: String,
    pub byte_len: usize,
    pub line_count: usize,
    pub head_excerpt: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum RefKind {
    Git,
    Local,
}

impl RefKind {
    pub fn prefix_char(self) -> char {
        match self {
            RefKind::Git => 'G',
            RefKind::Local => 'L',
        }
    }
}

const SOURCE_FILE_NAME: &str = "sources.md";
const NOTES_FILE_NAME: &str = "research_notes.md";

const KEY_FILES: &[&str] = &[
    "README.md",
    "README.rst",
    "README.txt",
    "README",
    "Readme.md",
    "readme.md",
    "AGENTS.md",
    "CONTRIBUTING.md",
    "ARCHITECTURE.md",
    "DESIGN.md",
    "ROADMAP.md",
    "CHANGELOG.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "COPYING",
    "docs/architecture.md",
    "docs/ARCHITECTURE.md",
    "docs/design.md",
    "docs/DESIGN.md",
    "docs/overview.md",
];

const MANIFEST_FILES: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "setup.cfg",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "Gemfile",
    "composer.json",
    "mix.exs",
    "stack.yaml",
    "cabal.project",
    "Package.swift",
    "Podfile",
    "pubspec.yaml",
    "deno.json",
    "bun.lockb",
    "tsconfig.json",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "Makefile",
    ".tool-versions",
];

const PRIMARY_SOURCE_DIRS: &[&str] = &[
    "src", "lib", "pkg", "internal", "app", "apps", "core", "cmd", "service",
    "services", "server", "frontend", "backend", "web", "api", "modules", "packages",
];

const CODE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "go", "py", "java", "kt", "kts", "scala",
    "swift", "rb", "php", "cs", "cpp", "cc", "cxx", "c", "h", "hpp", "m", "mm",
    "dart", "lua", "ex", "exs", "erl", "hs", "ml", "clj", "fs", "vue", "svelte",
];

#[derive(Debug, Clone)]
pub struct ParsedGitUrl {
    pub original: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
}

impl ParsedGitUrl {
    pub fn slug(&self) -> String {
        let host = sanitize_slug_part(&self.host);
        let owner = sanitize_slug_part(&self.owner);
        let repo = sanitize_slug_part(strip_dot_git(&self.repo));
        format!("{host}__{owner}__{repo}")
    }

    pub fn pretty(&self) -> String {
        format!(
            "{}/{}/{}",
            self.host,
            self.owner,
            strip_dot_git(&self.repo)
        )
    }
}

fn sanitize_slug_part(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if ch == '.' {
            out.push('.');
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("part");
    }
    out
}

fn strip_dot_git(name: &str) -> &str {
    name.strip_suffix(".git").unwrap_or(name)
}

pub fn parse_git_url(raw: &str) -> Option<ParsedGitUrl> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() < 2 {
            return None;
        }
        let owner = parts[0].to_string();
        let repo = parts[1..].join("/");
        return Some(ParsedGitUrl {
            original: raw.to_string(),
            host: host.to_string(),
            owner,
            repo,
        });
    }
    let scheme_split = trimmed
        .splitn(2, "://")
        .collect::<Vec<_>>();
    let host_and_path = if scheme_split.len() == 2 {
        scheme_split[1]
    } else {
        trimmed
    };
    let mut iter = host_and_path.splitn(2, '/');
    let host = iter.next()?.to_string();
    let path = iter.next().unwrap_or("");
    let parts: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    let owner = parts[0].to_string();
    let repo = parts[1..].join("/");
    Some(ParsedGitUrl {
        original: raw.to_string(),
        host,
        owner,
        repo,
    })
}

pub fn next_ref_id(root: &Path, kind: RefKind) -> Result<String> {
    let path = root.join(SOURCE_FILE_NAME);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let prefix = kind.prefix_char();
    let mut max_id = 0usize;
    for cap in REF_ID_BY_PREFIX_RE.captures_iter(&text) {
        if cap.get(1).map(|m| m.as_str()) != Some(prefix.to_string().as_str()) {
            continue;
        }
        if let Some(num) = cap.get(2).and_then(|m| m.as_str().parse::<usize>().ok()) {
            if num > max_id {
                max_id = num;
            }
        }
    }
    Ok(format!("[{}{}]", prefix, max_id + 1))
}

pub fn append_file(path: &Path, payload: &str) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {} for append", path.display()))?;
    file.write_all(payload.as_bytes())?;
    Ok(())
}

pub fn sources_path(root: &Path) -> PathBuf {
    root.join(SOURCE_FILE_NAME)
}

pub fn notes_path(root: &Path) -> PathBuf {
    root.join(NOTES_FILE_NAME)
}

pub fn read_head_utf8(path: &Path, max_chars: usize) -> Option<(String, bool)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() {
        return Some((String::new(), false));
    }
    let cap = max_chars.saturating_mul(4).min(bytes.len());
    let slice = &bytes[..cap];
    let text = String::from_utf8_lossy(slice).into_owned();
    let trimmed: String = text.chars().take(max_chars).collect();
    let truncated = bytes.len() > cap || text.chars().count() > max_chars;
    Some((trimmed, truncated))
}

pub fn detect_repo_metadata(repo_root: &Path, subpath: Option<&str>) -> RepoMetadata {
    let scan_root: PathBuf = match subpath {
        Some(sub) if !sub.is_empty() => repo_root.join(sub),
        _ => repo_root.to_path_buf(),
    };
    let mut meta = RepoMetadata::default();

    for candidate in KEY_FILES {
        let path = scan_root.join(candidate);
        if !path.is_file() {
            continue;
        }
        let lower = candidate.to_ascii_lowercase();
        if lower.contains("readme") && meta.readme_excerpt.is_none() {
            if let Some((text, _)) = read_head_utf8(&path, REFERENCE_HEAD_DOC_CHARS) {
                meta.readme_path = Some(rel_within(repo_root, &path));
                meta.readme_excerpt = Some(text);
            }
        } else if lower.contains("license") || lower.contains("copying") {
            meta.license_path = Some(rel_within(repo_root, &path));
            meta.license_name = guess_license_name(&path);
        } else if lower == "agents.md" {
            if let Some((text, _)) = read_head_utf8(&path, REFERENCE_HEAD_DOC_CHARS) {
                meta.agents_md = Some(text);
            }
        } else if lower.starts_with("contributing") {
            if let Some((text, _)) = read_head_utf8(&path, REFERENCE_HEAD_DOC_CHARS) {
                meta.contributing = Some(text);
            }
        } else if lower.contains("architecture") || lower.contains("design") {
            if meta.architecture_doc.is_none() {
                if let Some((text, _)) = read_head_utf8(&path, REFERENCE_HEAD_DOC_CHARS) {
                    meta.architecture_doc = Some(text);
                }
            }
        }
    }

    for manifest in MANIFEST_FILES {
        let path = scan_root.join(manifest);
        if !path.is_file() {
            continue;
        }
        let head = read_head_utf8(&path, 1_500).map(|(t, _)| t);
        meta.build_manifests.push(BuildManifest {
            name: (*manifest).to_string(),
            path: rel_within(repo_root, &path),
            head_excerpt: head,
        });
    }

    if let Ok(entries) = std::fs::read_dir(&scan_root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = match p.file_name().and_then(|n| n.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let lower = name.to_ascii_lowercase();
            if lower == "docs" || lower == "doc" || lower == "documentation" {
                meta.doc_dirs.push(rel_within(repo_root, &p));
            }
        }
    }

    meta.primary_languages = detect_primary_languages(&scan_root);
    meta
}

fn rel_within(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
        .replace('\\', "/")
}

fn guess_license_name(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let head_cap = bytes.len().min(2_048);
    let head = std::str::from_utf8(&bytes[..head_cap]).ok()?;
    let head_lower = head.to_ascii_lowercase();
    if head_lower.contains("apache license") && head_lower.contains("version 2.0") {
        Some("Apache-2.0".to_string())
    } else if head_lower.contains("mit license") {
        Some("MIT".to_string())
    } else if head_lower.contains("bsd 3-clause") {
        Some("BSD-3-Clause".to_string())
    } else if head_lower.contains("bsd 2-clause") {
        Some("BSD-2-Clause".to_string())
    } else if head_lower.contains("mozilla public license") {
        Some("MPL-2.0".to_string())
    } else if head_lower.contains("gnu general public license")
        && (head_lower.contains("version 3") || head_lower.contains("v3"))
    {
        Some("GPL-3.0".to_string())
    } else if head_lower.contains("gnu general public license")
        && (head_lower.contains("version 2") || head_lower.contains("v2"))
    {
        Some("GPL-2.0".to_string())
    } else if head_lower.contains("gnu lesser general public license") {
        Some("LGPL".to_string())
    } else if head_lower.contains("the unlicense") {
        Some("Unlicense".to_string())
    } else if head_lower.contains("creative commons") {
        Some("Creative Commons".to_string())
    } else {
        Some("Other".to_string())
    }
}

fn detect_primary_languages(root: &Path) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut visited = 0usize;
    const MAX_FILES_FOR_LANG_DETECTION: usize = 1_200;
    while let Some(dir) = stack.pop() {
        if visited >= MAX_FILES_FOR_LANG_DETECTION {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if visited >= MAX_FILES_FOR_LANG_DETECTION {
                break;
            }
            let p = entry.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if should_skip_dir(name) {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            visited += 1;
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_ascii_lowercase();
                if CODE_EXTENSIONS.iter().any(|x| *x == ext_lower) {
                    let lang = lang_label_from_ext(&ext_lower).to_string();
                    *counts.entry(lang).or_default() += 1;
                }
            }
        }
    }
    let mut items: Vec<(String, usize)> = counts.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1));
    items
        .into_iter()
        .take(5)
        .map(|(lang, count)| format!("{lang} ({count})"))
        .collect()
}

fn lang_label_from_ext(ext: &str) -> &'static str {
    match ext {
        "rs" => "Rust",
        "go" => "Go",
        "py" => "Python",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" => "JavaScript",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "scala" => "Scala",
        "rb" => "Ruby",
        "php" => "PHP",
        "cs" => "C#",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "m" | "mm" => "Objective-C",
        "dart" => "Dart",
        "lua" => "Lua",
        "ex" | "exs" => "Elixir",
        "erl" => "Erlang",
        "hs" => "Haskell",
        "ml" => "OCaml",
        "clj" => "Clojure",
        "fs" => "F#",
        "vue" => "Vue",
        "svelte" => "Svelte",
        _ => "Other",
    }
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".github"
            | ".gitlab"
            | ".idea"
            | ".vscode"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "bin"
            | "obj"
            | "vendor"
            | "third_party"
            | "third-party"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".cargo"
            | "tmp"
            | ".next"
            | ".turbo"
            | ".pnpm-store"
    )
}

pub fn scan_code_skeleton(
    repo_root: &Path,
    subpath: Option<&str>,
    max_files: usize,
) -> Vec<CodeFile> {
    let scan_root: PathBuf = match subpath {
        Some(sub) if !sub.is_empty() => repo_root.join(sub),
        _ => repo_root.to_path_buf(),
    };
    let mut candidates: Vec<PathBuf> = Vec::new();

    for primary in PRIMARY_SOURCE_DIRS {
        let dir = scan_root.join(primary);
        if !dir.is_dir() {
            continue;
        }
        collect_code_files_into(&dir, &mut candidates, 240);
        if candidates.len() >= max_files * 4 {
            break;
        }
    }

    if candidates.is_empty() {
        collect_code_files_into(&scan_root, &mut candidates, 240);
    }

    let mut scored: Vec<(PathBuf, usize)> = candidates
        .into_iter()
        .filter_map(|path| {
            let metadata = std::fs::metadata(&path).ok()?;
            if !metadata.is_file() {
                return None;
            }
            let bytes = metadata.len() as usize;
            if !(256..=250_000).contains(&bytes) {
                return None;
            }
            Some((path, bytes))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));

    let mut out: Vec<CodeFile> = Vec::new();
    for (path, byte_len) in scored {
        if out.len() >= max_files {
            break;
        }
        let head = match read_head_utf8(&path, REFERENCE_SKELETON_HEAD_CHARS) {
            Some(h) => h,
            None => continue,
        };
        let line_count = std::fs::read_to_string(&path)
            .map(|s| s.lines().count())
            .unwrap_or(0);
        out.push(CodeFile {
            relative_path: rel_within(repo_root, &path),
            byte_len,
            line_count,
            head_excerpt: head.0,
            truncated: head.1,
        });
    }
    out
}

fn collect_code_files_into(root: &Path, acc: &mut Vec<PathBuf>, cap: usize) {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if acc.len() >= cap {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if acc.len() >= cap {
                break;
            }
            let p = entry.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if should_skip_dir(name) {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_ascii_lowercase();
                if CODE_EXTENSIONS.iter().any(|x| *x == ext_lower) {
                    acc.push(p);
                }
            }
        }
    }
}

pub fn render_source_entry_for_reference(
    id: &str,
    title: &str,
    location_label: &str,
    location_value: &str,
    kind_label: &str,
    extras: &[(&'static str, String)],
    captured_at: &str,
    tags: Option<&[String]>,
    note: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("## {id}  -  {title}\n"));
    out.push_str(&format!("- Type: {kind_label}\n"));
    out.push_str(&format!("- {location_label}: {location_value}\n"));
    for (label, value) in extras {
        if value.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("- {label}: {value}\n"));
    }
    if let Some(tag_list) = tags {
        if !tag_list.is_empty() {
            out.push_str(&format!("- Tags: {}\n", tag_list.join(", ")));
        }
    }
    if let Some(n) = note {
        if !n.trim().is_empty() {
            out.push_str(&format!("- Note: {n}\n"));
        }
    }
    out.push_str(&format!("- Captured: {captured_at}\n\n"));
    out
}

pub fn render_research_notes_for_reference(
    id: &str,
    title: &str,
    kind_label: &str,
    location_label: &str,
    location_value: &str,
    metadata: &RepoMetadata,
    skeleton: &[CodeFile],
    captured_at: &str,
    note: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "### {id}  -  {title}\n- Type: {kind_label}\n- {location_label}: {location_value}\n- Captured: {captured_at}\n"
    ));
    if !metadata.primary_languages.is_empty() {
        out.push_str(&format!(
            "- Detected languages: {}\n",
            metadata.primary_languages.join(", ")
        ));
    }
    if let Some(license) = &metadata.license_name {
        out.push_str(&format!(
            "- License: {license}{}\n",
            metadata
                .license_path
                .as_ref()
                .map(|p| format!(" ({p})"))
                .unwrap_or_default()
        ));
    }
    if !metadata.build_manifests.is_empty() {
        let names: Vec<&str> = metadata
            .build_manifests
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        out.push_str(&format!("- Build manifests: {}\n", names.join(", ")));
    }
    if !metadata.doc_dirs.is_empty() {
        out.push_str(&format!("- Docs directories: {}\n", metadata.doc_dirs.join(", ")));
    }
    if let Some(n) = note {
        if !n.trim().is_empty() {
            out.push_str(&format!("- Curator note: {n}\n"));
        }
    }
    out.push('\n');
    if let Some(readme_text) = &metadata.readme_excerpt {
        if let Some(path) = &metadata.readme_path {
            out.push_str(&format!("#### README excerpt  -  `{path}`\n\n"));
        } else {
            out.push_str("#### README excerpt\n\n");
        }
        out.push_str("```text\n");
        out.push_str(readme_text);
        if !readme_text.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
    if let Some(arch_text) = &metadata.architecture_doc {
        out.push_str("#### ARCHITECTURE / DESIGN excerpt\n\n");
        out.push_str("```text\n");
        out.push_str(arch_text);
        if !arch_text.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
    for manifest in &metadata.build_manifests {
        if let Some(head) = &manifest.head_excerpt {
            out.push_str(&format!("#### `{}` head\n\n", manifest.path));
            out.push_str("```text\n");
            out.push_str(head);
            if !head.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
    }
    if !skeleton.is_empty() {
        out.push_str("#### Key source skeleton (largest in-scope files)\n\n");
        for file in skeleton {
            out.push_str(&format!(
                "##### `{}`  -  {} bytes, {} lines{}\n\n",
                file.relative_path,
                file.byte_len,
                file.line_count,
                if file.truncated { " (head excerpt)" } else { "" }
            ));
            out.push_str("```text\n");
            out.push_str(&file.head_excerpt);
            if !file.head_excerpt.ends_with('\n') {
                out.push('\n');
            }
            if file.truncated {
                out.push_str("... [truncated; re-open the file for the rest] ...\n");
            }
            out.push_str("```\n\n");
        }
    }
    out
}

pub fn iso_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
