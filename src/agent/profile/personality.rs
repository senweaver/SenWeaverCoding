// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::fmt::Write;
use std::path::{Path, PathBuf};

const MAX_FILE_CHARS: usize = 20_000;

const PERSONALITY_FILES: &[&str] = &[
    "SOUL.md",
    "IDENTITY.md",
    "USER.md",
    "AGENTS.md",
    "TOOLS.md",
    "HEARTBEAT.md",
    "BOOTSTRAP.md",
    "MEMORY.md",
];

#[derive(Debug, Clone)]
pub struct PersonalityFile {

    pub name: String,

    pub content: String,

    pub truncated: bool,

    pub path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct PersonalityProfile {

    pub files: Vec<PersonalityFile>,

    pub missing: Vec<String>,
}

impl PersonalityProfile {

    pub fn get(&self, name: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.content.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for file in &self.files {
            let _ = writeln!(out, "### {}\n", file.name);
            out.push_str(&file.content);
            if file.truncated {
                let _ = writeln!(
                    out,
                    "\n\n[... truncated at {MAX_FILE_CHARS} chars  -  use `read` for full file]\n"
                );
            } else {
                out.push_str("\n\n");
            }
        }
        out
    }
}

const PERSONALITY_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);

struct CachedPersonality {
    checked_at: std::time::Instant,
    fingerprint: u64,
    profile: PersonalityProfile,
}

fn personality_cache()
-> &'static parking_lot::Mutex<std::collections::HashMap<PathBuf, CachedPersonality>> {
    static CACHE: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<PathBuf, CachedPersonality>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

fn personality_fingerprint(workspace_dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for &filename in PERSONALITY_FILES {
        let path = workspace_dir.join(filename);
        filename.hash(&mut hasher);
        match std::fs::metadata(&path) {
            Ok(meta) => {
                meta.len().hash(&mut hasher);
                if let Ok(modified) = meta.modified() {
                    modified.hash(&mut hasher);
                }
            }
            Err(_) => 0u8.hash(&mut hasher),
        }
    }
    let rules_dir = workspace_dir.join(".cursor").join("rules");
    if rules_dir.is_dir() {
        let mut paths: Vec<PathBuf> = Vec::new();
        walk_cursor_rule_files(&rules_dir, 0, &mut paths);
        paths.sort();
        for path in paths {
            path.hash(&mut hasher);
            if let Ok(meta) = std::fs::metadata(&path) {
                meta.len().hash(&mut hasher);
                if let Ok(modified) = meta.modified() {
                    modified.hash(&mut hasher);
                }
            }
        }
    }
    hasher.finish()
}

pub fn load_personality(workspace_dir: &Path) -> PersonalityProfile {
    {
        let cache = personality_cache().lock();
        if let Some(cached) = cache.get(workspace_dir) {
            if cached.checked_at.elapsed() < PERSONALITY_CACHE_TTL {
                return cached.profile.clone();
            }
        }
    }

    let fingerprint = personality_fingerprint(workspace_dir);
    {
        let mut cache = personality_cache().lock();
        if let Some(cached) = cache.get_mut(workspace_dir) {
            if cached.fingerprint == fingerprint {
                cached.checked_at = std::time::Instant::now();
                return cached.profile.clone();
            }
        }
    }

    let mut profile = load_personality_files(workspace_dir, PERSONALITY_FILES);
    append_cursor_rules(&mut profile, workspace_dir);
    personality_cache().lock().insert(
        workspace_dir.to_path_buf(),
        CachedPersonality {
            checked_at: std::time::Instant::now(),
            fingerprint,
            profile: profile.clone(),
        },
    );
    profile
}

fn append_cursor_rules(profile: &mut PersonalityProfile, workspace_dir: &Path) {
    let rules_dir = workspace_dir.join(".cursor").join("rules");
    if !rules_dir.is_dir() {
        return;
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    walk_cursor_rule_files(&rules_dir, 0, &mut paths);
    paths.sort();
    for path in paths {
        let display_name = path
            .strip_prefix(workspace_dir)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let (content, truncated) = truncate_content(trimmed);
                profile.files.push(PersonalityFile {
                    name: display_name,
                    content,
                    truncated,
                    path,
                });
            }
            Err(err) => {
                tracing::debug!(
                    target: "agent.personality",
                    path = %path.display(),
                    error = %err,
                    "skipped unreadable cursor rule file"
                );
            }
        }
    }
}

fn walk_cursor_rule_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 1 {
                walk_cursor_rule_files(&path, depth + 1, out);
            }
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext_lc = ext.to_ascii_lowercase();
        if ext_lc == "mdc" || ext_lc == "md" {
            out.push(path);
        }
    }
}

pub fn load_personality_files(workspace_dir: &Path, filenames: &[&str]) -> PersonalityProfile {
    let mut profile = PersonalityProfile::default();

    for &filename in filenames {
        let path = workspace_dir.join(filename);
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    profile.missing.push(filename.to_string());
                    continue;
                }
                let (content, truncated) = truncate_content(trimmed);
                profile.files.push(PersonalityFile {
                    name: filename.to_string(),
                    content,
                    truncated,
                    path,
                });
            }
            Err(_) => {
                profile.missing.push(filename.to_string());
            }
        }
    }

    profile
}

fn truncate_content(content: &str) -> (String, bool) {
    if content.chars().count() <= MAX_FILE_CHARS {
        return (content.to_string(), false);
    }
    let truncated = content
        .char_indices()
        .nth(MAX_FILE_CHARS)
        .map(|(idx, _)| &content[..idx])
        .unwrap_or(content);
    (truncated.to_string(), true)
}
