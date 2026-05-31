// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use anyhow::{Context, Result};
use directories::UserDirs;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use zip::ZipArchive;

mod audit;
#[cfg(feature = "skill-creation")]
pub mod creator;
#[cfg(feature = "skill-creation")]
pub mod improver;
pub mod testing;

const OPEN_SKILLS_REPO_URL: &str = "https://github.com/besoeasy/open-skills";
const OPEN_SKILLS_SYNC_MARKER: &str = ".senweavercoding-open-skills-sync";
const OPEN_SKILLS_SYNC_INTERVAL_SECS: u64 = 60 * 60 * 24 * 7;

const CLAWHUB_DOMAIN: &str = "clawhub.ai";
const CLAWHUB_WWW_DOMAIN: &str = "www.clawhub.ai";
const CLAWHUB_DOWNLOAD_API: &str = "https://clawhub.ai/api/v1/download";
const MAX_CLAWHUB_ZIP_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tools: Vec<SkillTool>,
    #[serde(default)]
    pub prompts: Vec<String>,
    #[serde(skip)]
    pub location: Option<PathBuf>,
    #[serde(default)]
    pub always_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTool {
    pub name: String,
    pub description: String,

    pub kind: String,

    pub command: String,
    #[serde(default)]
    pub args: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillManifest {
    skill: SkillMeta,
    #[serde(default)]
    tools: Vec<SkillTool>,
    #[serde(default)]
    prompts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillMeta {
    name: String,
    description: String,
    #[serde(default = "default_version")]
    version: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, alias = "alwaysApply", alias = "alwaysapply")]
    always_apply: bool,
}

#[derive(Debug, Clone, Default)]
struct SkillMarkdownMeta {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    author: Option<String>,
    tags: Vec<String>,
    always_apply: Option<bool>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn warn_skipped_skill(path: &Path, summary: &str, allow_scripts: bool) {
    let scripts_blocked = summary.contains("script-like files are blocked");
    if scripts_blocked && !allow_scripts {
        tracing::warn!(
            "skipping skill directory {}: {summary}. \
             To allow script files in skills, set `skills.allow_scripts = true` in your config.",
            path.display(),
        );
        eprintln!(
            "warning: skill '{}' was skipped because it contains script files. \
             Set `skills.allow_scripts = true` in your sen config to enable it.",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
        );
    } else {
        tracing::warn!(
            "skipping insecure skill directory {}: {summary}",
            path.display(),
        );
    }
}

pub fn load_skills(workspace_dir: &Path) -> Vec<Skill> {
    load_skills_with_open_skills_config(workspace_dir, None, None, None)
}

pub fn discover_skills(workspace_dir: &Path, config: &crate::config::Config) -> Vec<Skill> {
    load_skills_with_open_skills_config(
        workspace_dir,
        Some(config.skills.open_skills_enabled),
        config.skills.open_skills_dir.as_deref(),
        Some(config.skills.allow_scripts),
    )
}

pub fn filter_skills_by_disabled_list(mut skills: Vec<Skill>, disabled: &[String]) -> Vec<Skill> {
    if disabled.is_empty() {
        return skills;
    }
    let blocked: HashSet<String> = disabled
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if blocked.is_empty() {
        return skills;
    }
    skills.retain(|s| !blocked.contains(&s.name));
    skills
}

pub fn load_skills_with_config(workspace_dir: &Path, config: &crate::config::Config) -> Vec<Skill> {
    let skills = discover_skills(workspace_dir, config);
    filter_skills_by_disabled_list(skills, &config.skills.disabled_skills)
}

pub fn load_skills_with_open_skills_settings(
    workspace_dir: &Path,
    open_skills_enabled: bool,
    open_skills_dir: Option<&str>,
) -> Vec<Skill> {
    load_skills_with_open_skills_config(
        workspace_dir,
        Some(open_skills_enabled),
        open_skills_dir,
        None,
    )
}

fn load_skills_with_open_skills_config(
    workspace_dir: &Path,
    config_open_skills_enabled: Option<bool>,
    config_open_skills_dir: Option<&str>,
    config_allow_scripts: Option<bool>,
) -> Vec<Skill> {
    let mut skills = Vec::new();
    let allow_scripts = config_allow_scripts.unwrap_or(false);

    if let Some(open_skills_dir) =
        ensure_open_skills_repo(config_open_skills_enabled, config_open_skills_dir)
    {
        skills.extend(load_open_skills(&open_skills_dir, allow_scripts));
    }

    skills.extend(load_user_skills(allow_scripts));
    skills.extend(load_workspace_skills(workspace_dir, allow_scripts));

    let mut seen: HashSet<String> = HashSet::new();
    let mut deduped: Vec<Skill> = Vec::with_capacity(skills.len());
    for skill in skills.into_iter().rev() {
        if seen.insert(skill.name.clone()) {
            deduped.push(skill);
        }
    }
    deduped.reverse();
    deduped
}

fn load_workspace_skills(workspace_dir: &Path, allow_scripts: bool) -> Vec<Skill> {
    let skills_dir = workspace_dir.join("skills");
    load_skills_from_directory(&skills_dir, allow_scripts)
}

fn load_user_skills(allow_scripts: bool) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for candidate in user_skill_dirs() {
        let canonical = std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
        if !seen.insert(canonical) {
            continue;
        }
        if !candidate.is_dir() {
            continue;
        }
        out.extend(load_skills_from_directory(&candidate, allow_scripts));
    }
    out
}

fn user_skill_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(explicit) = std::env::var("AGENTS_SKILLS_DIR") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            dirs.push(PathBuf::from(trimmed));
        }
    }
    if let Some(home) = home_dir_for_skills() {
        dirs.push(home.join(".agents").join("skills"));
        dirs.push(home.join(".senweavercoding").join("skills"));
    }
    dirs
}

fn home_dir_for_skills() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    None
}

pub fn load_skills_from_directory(skills_dir: &Path, allow_scripts: bool) -> Vec<Skill> {
    if !skills_dir.exists() {
        return Vec::new();
    }

    let mut skills = Vec::new();

    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return skills;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }

        match audit::audit_skill_directory_with_options(
            &path,
            audit::SkillAuditOptions { allow_scripts },
        ) {
            Ok(report) if report.is_clean() => {}
            Ok(report) => {
                let summary = report.summary();
                warn_skipped_skill(&path, &summary, allow_scripts);
                continue;
            }
            Err(err) => {
                tracing::warn!(
                    "skipping unauditable skill directory {}: {err}",
                    path.display()
                );
                continue;
            }
        }

        let manifest_path = path.join("SKILL.toml");
        let md_path = path.join("SKILL.md");

        if manifest_path.exists() {
            if let Ok(skill) = load_skill_toml(&manifest_path) {
                skills.push(skill);
            }
        } else if md_path.exists() {
            if let Ok(skill) = load_skill_md(&md_path, &path) {
                skills.push(skill);
            }
        }
    }

    skills
}

fn finalize_open_skill(mut skill: Skill) -> Skill {
    if !skill.tags.iter().any(|tag| tag == "open-skills") {
        skill.tags.push("open-skills".to_string());
    }
    if skill.author.is_none() {
        skill.author = Some("besoeasy/open-skills".to_string());
    }
    skill
}

fn load_open_skills_from_directory(skills_dir: &Path, allow_scripts: bool) -> Vec<Skill> {
    if !skills_dir.exists() {
        return Vec::new();
    }

    let mut skills = Vec::new();

    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return skills;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }

        match audit::audit_skill_directory_with_options(
            &path,
            audit::SkillAuditOptions { allow_scripts },
        ) {
            Ok(report) if report.is_clean() => {}
            Ok(report) => {
                let summary = report.summary();
                warn_skipped_skill(&path, &summary, allow_scripts);
                continue;
            }
            Err(err) => {
                tracing::warn!(
                    "skipping unauditable open-skill directory {}: {err}",
                    path.display()
                );
                continue;
            }
        }

        let manifest_path = path.join("SKILL.toml");
        let md_path = path.join("SKILL.md");

        if manifest_path.exists() {
            if let Ok(skill) = load_skill_toml(&manifest_path) {
                skills.push(finalize_open_skill(skill));
            }
        } else if md_path.exists() {
            if let Ok(skill) = load_open_skill_md(&md_path) {
                skills.push(skill);
            }
        }
    }

    skills
}

fn load_open_skills(repo_dir: &Path, allow_scripts: bool) -> Vec<Skill> {

    let nested_skills_dir = repo_dir.join("skills");
    if nested_skills_dir.is_dir() {
        return load_open_skills_from_directory(&nested_skills_dir, allow_scripts);
    }

    let mut skills = Vec::new();

    let Ok(entries) = std::fs::read_dir(repo_dir) else {
        return skills;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let is_markdown = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if !is_markdown {
            continue;
        }

        let is_readme = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("README.md"));
        if is_readme {
            continue;
        }

        match audit::audit_open_skill_markdown(&path, repo_dir) {
            Ok(report) if report.is_clean() => {}
            Ok(report) => {
                tracing::warn!(
                    "skipping insecure open-skill file {}: {}",
                    path.display(),
                    report.summary()
                );
                continue;
            }
            Err(err) => {
                tracing::warn!(
                    "skipping unauditable open-skill file {}: {err}",
                    path.display()
                );
                continue;
            }
        }

        if let Ok(skill) = load_open_skill_md(&path) {
            skills.push(skill);
        }
    }

    skills
}

fn parse_open_skills_enabled(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn open_skills_enabled_from_sources(
    config_open_skills_enabled: Option<bool>,
    env_override: Option<&str>,
) -> bool {
    if let Some(raw) = env_override {
        if let Some(enabled) = parse_open_skills_enabled(raw) {
            return enabled;
        }
        if !raw.trim().is_empty() {
            tracing::warn!(
                "Ignoring invalid SEN_OPEN_SKILLS_ENABLED (valid: 1|0|true|false|yes|no|on|off)"
            );
        }
    }

    config_open_skills_enabled.unwrap_or(false)
}

fn open_skills_enabled(config_open_skills_enabled: Option<bool>) -> bool {
    let env_override = std::env::var("SEN_OPEN_SKILLS_ENABLED").ok();
    open_skills_enabled_from_sources(config_open_skills_enabled, env_override.as_deref())
}

fn resolve_open_skills_dir_from_sources(
    env_dir: Option<&str>,
    config_dir: Option<&str>,
    home_dir: Option<&Path>,
) -> Option<PathBuf> {
    let parse_dir = |raw: &str| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    };

    if let Some(env_dir) = env_dir.and_then(parse_dir) {
        return Some(env_dir);
    }
    if let Some(config_dir) = config_dir.and_then(parse_dir) {
        return Some(config_dir);
    }
    home_dir.map(|home| home.join("open-skills"))
}

fn resolve_open_skills_dir(config_open_skills_dir: Option<&str>) -> Option<PathBuf> {
    let env_dir = std::env::var("SEN_OPEN_SKILLS_DIR").ok();
    let home_dir = UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
    resolve_open_skills_dir_from_sources(
        env_dir.as_deref(),
        config_open_skills_dir,
        home_dir.as_deref(),
    )
}

fn ensure_open_skills_repo(
    config_open_skills_enabled: Option<bool>,
    config_open_skills_dir: Option<&str>,
) -> Option<PathBuf> {
    if !open_skills_enabled(config_open_skills_enabled) {
        return None;
    }

    let repo_dir = resolve_open_skills_dir(config_open_skills_dir)?;

    if !repo_dir.exists() {
        if !clone_open_skills_repo(&repo_dir) {
            return None;
        }
        let _ = mark_open_skills_synced(&repo_dir);
        return Some(repo_dir);
    }

    if should_sync_open_skills(&repo_dir) {
        if pull_open_skills_repo(&repo_dir) {
            let _ = mark_open_skills_synced(&repo_dir);
        } else {
            tracing::warn!(
                "open-skills update failed; using local copy from {}",
                repo_dir.display()
            );
        }
    }

    Some(repo_dir)
}

fn clone_open_skills_repo(repo_dir: &Path) -> bool {
    if let Some(parent) = repo_dir.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                "failed to create open-skills parent directory {}: {err}",
                parent.display()
            );
            return false;
        }
    }

    let output = crate::util::hidden_sync_command("git")
        .args(["clone", "--depth", "1", OPEN_SKILLS_REPO_URL])
        .arg(repo_dir)
        .output();

    match output {
        Ok(result) if result.status.success() => {
            tracing::info!("initialized open-skills at {}", repo_dir.display());
            true
        }
        Ok(result) => {
            let stderr = String::from_utf8_lossy(&result.stderr);
            tracing::warn!("failed to clone open-skills: {stderr}");
            false
        }
        Err(err) => {
            tracing::warn!("failed to run git clone for open-skills: {err}");
            false
        }
    }
}

fn pull_open_skills_repo(repo_dir: &Path) -> bool {

    if !repo_dir.join(".git").exists() {
        return true;
    }

    let output = crate::util::hidden_sync_command("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["pull", "--ff-only"])
        .output();

    match output {
        Ok(result) if result.status.success() => true,
        Ok(result) => {
            let stderr = String::from_utf8_lossy(&result.stderr);
            tracing::warn!("failed to pull open-skills updates: {stderr}");
            false
        }
        Err(err) => {
            tracing::warn!("failed to run git pull for open-skills: {err}");
            false
        }
    }
}

fn should_sync_open_skills(repo_dir: &Path) -> bool {
    let marker = repo_dir.join(OPEN_SKILLS_SYNC_MARKER);
    let Ok(metadata) = std::fs::metadata(marker) else {
        return true;
    };
    let Ok(modified_at) = metadata.modified() else {
        return true;
    };
    let Ok(age) = SystemTime::now().duration_since(modified_at) else {
        return true;
    };

    age >= Duration::from_secs(OPEN_SKILLS_SYNC_INTERVAL_SECS)
}

fn mark_open_skills_synced(repo_dir: &Path) -> Result<()> {
    std::fs::write(repo_dir.join(OPEN_SKILLS_SYNC_MARKER), b"synced")?;
    Ok(())
}

fn load_skill_toml(path: &Path) -> Result<Skill> {
    let content = std::fs::read_to_string(path)?;
    let manifest: SkillManifest = toml::from_str(&content)?;

    Ok(Skill {
        always_apply: manifest.skill.always_apply,
        name: manifest.skill.name,
        description: manifest.skill.description,
        version: manifest.skill.version,
        author: manifest.skill.author,
        tags: manifest.skill.tags,
        tools: manifest.tools,
        prompts: manifest.prompts,
        location: Some(path.to_path_buf()),
    })
}

fn load_skill_md(path: &Path, dir: &Path) -> Result<Skill> {
    let content = std::fs::read_to_string(path)?;
    let parsed = parse_skill_markdown(&content);
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(Skill {
        always_apply: parsed.meta.always_apply.unwrap_or(false),
        name: parsed.meta.name.unwrap_or(name),
        description: parsed
            .meta
            .description
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| extract_description(&parsed.body)),
        version: parsed.meta.version.unwrap_or_else(default_version),
        author: parsed.meta.author,
        tags: parsed.meta.tags,
        tools: Vec::new(),
        prompts: vec![parsed.body],
        location: Some(path.to_path_buf()),
    })
}

fn load_open_skill_md(path: &Path) -> Result<Skill> {
    let content = std::fs::read_to_string(path)?;
    let parsed = parse_skill_markdown(&content);
    let file_stem = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("open-skill")
        .to_string();
    let name = if file_stem.eq_ignore_ascii_case("skill") {
        path.parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or(&file_stem)
            .to_string()
    } else {
        file_stem
    };
    Ok(finalize_open_skill(Skill {
        always_apply: parsed.meta.always_apply.unwrap_or(false),
        name: parsed.meta.name.unwrap_or(name),
        description: parsed
            .meta
            .description
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| extract_description(&parsed.body)),
        version: parsed
            .meta
            .version
            .unwrap_or_else(|| "open-skills".to_string()),
        author: parsed
            .meta
            .author
            .or_else(|| Some("besoeasy/open-skills".to_string())),
        tags: parsed.meta.tags,
        tools: Vec::new(),
        prompts: vec![parsed.body],
        location: Some(path.to_path_buf()),
    }))
}

struct ParsedSkillMarkdown {
    meta: SkillMarkdownMeta,
    body: String,
}

fn parse_skill_markdown(content: &str) -> ParsedSkillMarkdown {
    if let Some((frontmatter, body)) = split_skill_frontmatter(content) {
        let meta = parse_simple_frontmatter(&frontmatter);
        return ParsedSkillMarkdown { meta, body };
    }

    ParsedSkillMarkdown {
        meta: SkillMarkdownMeta::default(),
        body: content.to_string(),
    }
}

fn parse_simple_frontmatter(s: &str) -> SkillMarkdownMeta {
    let mut meta = SkillMarkdownMeta::default();
    let mut collecting_tags = false;
    for line in s.lines() {

        if collecting_tags {
            let trimmed = line.trim();
            if let Some(item) = trimmed.strip_prefix("- ") {
                let tag = item.trim().trim_matches('"').trim_matches('\'');
                if !tag.is_empty() {
                    meta.tags.push(tag.to_string());
                }
                continue;
            }

            collecting_tags = false;
        }
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim().trim_matches('"').trim_matches('\'');
        match key {
            "name" => meta.name = Some(val.to_string()),
            "description" => meta.description = Some(val.to_string()),
            "version" => meta.version = Some(val.to_string()),
            "author" => meta.author = Some(val.to_string()),
            "always_apply" | "alwaysApply" | "alwaysapply" => {
                meta.always_apply = match val.to_ascii_lowercase().as_str() {
                    "true" | "yes" | "1" | "on" => Some(true),
                    "false" | "no" | "0" | "off" => Some(false),
                    _ => None,
                };
            }
            "tags" => {
                if val.is_empty() {

                    collecting_tags = true;
                } else {

                    let val = val.trim_start_matches('[').trim_end_matches(']');
                    meta.tags = val
                        .split(',')
                        .map(|t| t.trim().trim_matches('"').trim_matches('\'').to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
            }
            _ => {}
        }
    }
    meta
}

fn split_skill_frontmatter(content: &str) -> Option<(String, String)> {
    let normalized = content.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    if let Some(idx) = rest.find("\n---\n") {
        let frontmatter = rest[..idx].to_string();
        let body = rest[idx + 5..].to_string();
        return Some((frontmatter, body));
    }
    if let Some(frontmatter) = rest.strip_suffix("\n---") {
        return Some((frontmatter.to_string(), String::new()));
    }
    None
}

fn extract_description(content: &str) -> String {
    content
        .lines()
        .find(|line| !line.starts_with('#') && !line.trim().is_empty())
        .unwrap_or("No description")
        .trim()
        .to_string()
}

fn append_xml_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
}

fn write_xml_text_element(out: &mut String, indent: usize, tag: &str, value: &str) {
    for _ in 0..indent {
        out.push(' ');
    }
    out.push('<');
    out.push_str(tag);
    out.push('>');
    append_xml_escaped(out, value);
    out.push_str("</");
    out.push_str(tag);
    out.push_str(">\n");
}

fn resolve_skill_location(skill: &Skill, workspace_dir: &Path) -> PathBuf {
    skill.location.clone().unwrap_or_else(|| {
        workspace_dir
            .join("skills")
            .join(&skill.name)
            .join("SKILL.md")
    })
}

fn render_skill_location(skill: &Skill, workspace_dir: &Path, prefer_relative: bool) -> String {
    let location = resolve_skill_location(skill, workspace_dir);
    if prefer_relative {
        if let Ok(relative) = location.strip_prefix(workspace_dir) {
            return relative.display().to_string();
        }
    }
    location.display().to_string()
}

pub fn skills_to_prompt(skills: &[Skill], workspace_dir: &Path) -> String {
    skills_to_prompt_with_mode(
        skills,
        workspace_dir,
        crate::config::SkillsPromptInjectionMode::Full,
    )
}

pub fn skills_to_prompt_with_mode(
    skills: &[Skill],
    workspace_dir: &Path,
    mode: crate::config::SkillsPromptInjectionMode,
) -> String {
    use std::fmt::Write;

    if skills.is_empty() {
        return String::new();
    }

    let mode_is_compact = matches!(mode, crate::config::SkillsPromptInjectionMode::Compact);

    let mut prompt = match mode {
        crate::config::SkillsPromptInjectionMode::Full => String::from(
            "## Available Skills\n\n\
             Skill instructions and tool metadata are preloaded below.\n\
             Follow these instructions directly; do not read skill files at runtime unless the user asks.\n\n\
             <available_skills>\n",
        ),
        crate::config::SkillsPromptInjectionMode::Compact => String::from(
            "## Available Skills\n\n\
             To keep the system prompt compact, most skills below expose only their \
             `<name>`, `<description>` and `<location>`. Two rules govern usage:\n\
             1. Skills marked `always_apply=\"true\"` ARE fully expanded below; treat their \
                instructions as binding for every relevant request.\n\
             2. For all other skills, the moment a `<description>` even partially matches the \
                user's task, call `read_skill(name=\"...\")` to load the full skill file \
                BEFORE acting. Never guess at a skill's contents from its description alone.\n\n\
             <available_skills>\n",
        ),
    };

    for skill in skills {
        let expand_full = !mode_is_compact || skill.always_apply;
        let location_relative = mode_is_compact && !skill.always_apply;

        let _ = writeln!(prompt, "  <skill>");
        write_xml_text_element(&mut prompt, 4, "name", &skill.name);
        write_xml_text_element(&mut prompt, 4, "description", &skill.description);
        let location = render_skill_location(skill, workspace_dir, location_relative);
        write_xml_text_element(&mut prompt, 4, "location", &location);
        if skill.always_apply {
            write_xml_text_element(&mut prompt, 4, "always_apply", "true");
        }

        if expand_full && !skill.prompts.is_empty() {
            let _ = writeln!(prompt, "    <instructions>");
            for instruction in &skill.prompts {
                write_xml_text_element(&mut prompt, 6, "instruction", instruction);
            }
            let _ = writeln!(prompt, "    </instructions>");
        }

        if expand_full && !skill.tools.is_empty() {
            let registered: Vec<_> = skill
                .tools
                .iter()
                .filter(|t| matches!(t.kind.as_str(), "shell" | "script" | "http"))
                .collect();
            let unregistered: Vec<_> = skill
                .tools
                .iter()
                .filter(|t| !matches!(t.kind.as_str(), "shell" | "script" | "http"))
                .collect();

            if !registered.is_empty() {
                let _ = writeln!(
                    prompt,
                    "    <callable_tools hint=\"These are registered as callable tool specs. Invoke them directly by name ({{}}.{{}}) instead of using shell.\">"
                );
                for tool in &registered {
                    let _ = writeln!(prompt, "      <tool>");
                    write_xml_text_element(
                        &mut prompt,
                        8,
                        "name",
                        &format!("{}.{}", skill.name, tool.name),
                    );
                    write_xml_text_element(&mut prompt, 8, "description", &tool.description);
                    let _ = writeln!(prompt, "      </tool>");
                }
                let _ = writeln!(prompt, "    </callable_tools>");
            }

            if !unregistered.is_empty() {
                let _ = writeln!(prompt, "    <tools>");
                for tool in &unregistered {
                    let _ = writeln!(prompt, "      <tool>");
                    write_xml_text_element(&mut prompt, 8, "name", &tool.name);
                    write_xml_text_element(&mut prompt, 8, "description", &tool.description);
                    write_xml_text_element(&mut prompt, 8, "kind", &tool.kind);
                    let _ = writeln!(prompt, "      </tool>");
                }
                let _ = writeln!(prompt, "    </tools>");
            }
        }

        let _ = writeln!(prompt, "  </skill>");
    }

    prompt.push_str("</available_skills>");
    prompt
}

pub fn skills_to_tools(
    skills: &[Skill],
    security: std::sync::Arc<crate::security::SecurityPolicy>,
) -> Vec<Box<dyn crate::tools::traits::Tool>> {
    let mut tools: Vec<Box<dyn crate::tools::traits::Tool>> = Vec::new();
    for skill in skills {
        for tool in &skill.tools {
            match tool.kind.as_str() {
                "shell" | "script" => {
                    tools.push(Box::new(crate::tools::skill::tool::SkillShellTool::new(
                        &skill.name,
                        tool,
                        security.clone(),
                    )));
                }
                "http" => {
                    tools.push(Box::new(crate::tools::skill::http::SkillHttpTool::new(
                        &skill.name,
                        tool,
                    )));
                }
                other => {
                    tracing::warn!(
                        "Unknown skill tool kind '{}' for {}.{}, skipping",
                        other,
                        skill.name,
                        tool.name
                    );
                }
            }
        }
    }
    tools
}

pub fn skills_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("skills")
}

pub fn init_skills_dir(workspace_dir: &Path) -> Result<()> {
    let dir = skills_dir(workspace_dir);
    std::fs::create_dir_all(&dir)?;

    let readme = dir.join("README.md");
    if !readme.exists() {
        std::fs::write(
            &readme,
            "# SenWeaverCoding Skills\n\n\
             Each subdirectory is a skill. Create a `SKILL.toml` or `SKILL.md` file inside.\n\n\
             ## SKILL.toml format\n\n\
             ```toml\n\
             [skill]\n\
             name = \"my-skill\"\n\
             description = \"What this skill does\"\n\
             version = \"0.1.0\"\n\
             author = \"your-name\"\n\
             tags = [\"productivity\", \"automation\"]\n\n\
             [[tools]]\n\
             name = \"my_tool\"\n\
             description = \"What this tool does\"\n\
             kind = \"shell\"\n\
             command = \"echo hello\"\n\
             ```\n\n\
             ## SKILL.md format (simpler)\n\n\
             Just write a markdown file with instructions for the agent.\n\
             Optional YAML frontmatter is supported for `name`, `description`, `version`, `author`, and `tags`.\n\
             The agent will read it and follow the instructions.\n\n\
             ## Installing community skills\n\n\
             ```bash\n\
             sen skills install <source>\n\
             sen skills list\n\
             ```\n",
        )?;
    }

    Ok(())
}

fn is_clawhub_host(host: &str) -> bool {
    host.eq_ignore_ascii_case(CLAWHUB_DOMAIN) || host.eq_ignore_ascii_case(CLAWHUB_WWW_DOMAIN)
}

fn parse_clawhub_url(source: &str) -> Option<Url> {
    let parsed = Url::parse(source).ok()?;
    match parsed.scheme() {
        "https" | "http" => {}
        _ => return None,
    }

    if !parsed.host_str().is_some_and(is_clawhub_host) {
        return None;
    }

    Some(parsed)
}

fn is_clawhub_source(source: &str) -> bool {
    if source.starts_with("clawhub:") {
        return true;
    }
    parse_clawhub_url(source).is_some()
}

fn clawhub_download_url(source: &str) -> Result<String> {

    if let Some(slug) = source.strip_prefix("clawhub:") {
        let slug = slug.trim().trim_end_matches('/');
        if slug.is_empty() || slug.contains('/') {
            anyhow::bail!(
                "invalid clawhub source '{}': expected 'clawhub:<slug>' (no slashes in slug)",
                source
            );
        }
        return Ok(format!("{CLAWHUB_DOWNLOAD_API}?slug={slug}"));
    }

    if let Some(parsed) = parse_clawhub_url(source) {
        let path = parsed
            .path_segments()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("/");

        if path.is_empty() {
            anyhow::bail!("could not extract slug from ClawhHub URL: {source}");
        }

        return Ok(format!("{CLAWHUB_DOWNLOAD_API}?slug={path}"));
    }

    anyhow::bail!("unrecognised ClawhHub source format: {source}")
}

fn normalize_skill_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c == '-' { '_' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

fn clawhub_skill_dir_name(source: &str) -> Result<String> {
    if let Some(slug) = source.strip_prefix("clawhub:") {
        let slug = slug.trim().trim_end_matches('/');
        let base = slug.rsplit('/').next().unwrap_or(slug);
        let name = normalize_skill_name(base);
        return Ok(if name.is_empty() {
            "skill".to_string()
        } else {
            name
        });
    }

    let parsed = parse_clawhub_url(source)
        .ok_or_else(|| anyhow::anyhow!("invalid clawhub URL: {source}"))?;

    let path = parsed
        .path_segments()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let base = path.last().copied().unwrap_or("skill");
    let name = normalize_skill_name(base);
    Ok(if name.is_empty() {
        "skill".to_string()
    } else {
        name
    })
}

fn is_git_source(source: &str) -> bool {

    if is_clawhub_source(source) {
        return false;
    }
    is_git_scheme_source(source, "https://")
        || is_git_scheme_source(source, "http://")
        || is_git_scheme_source(source, "ssh://")
        || is_git_scheme_source(source, "git://")
        || is_git_scp_source(source)
}

fn is_git_scheme_source(source: &str, scheme: &str) -> bool {
    let Some(rest) = source.strip_prefix(scheme) else {
        return false;
    };
    if rest.is_empty() || rest.starts_with('/') {
        return false;
    }

    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty()
}

fn is_git_scp_source(source: &str) -> bool {

    let Some((user_host, remote_path)) = source.split_once(':') else {
        return false;
    };
    if remote_path.is_empty() {
        return false;
    }
    if source.contains("://") {
        return false;
    }

    let Some((user, host)) = user_host.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && !host.is_empty()
        && !user.contains('/')
        && !user.contains('\\')
        && !host.contains('/')
        && !host.contains('\\')
}

fn snapshot_skill_children(skills_path: &Path) -> Result<HashSet<PathBuf>> {
    let mut paths = HashSet::new();
    for entry in std::fs::read_dir(skills_path)? {
        let entry = entry?;
        paths.insert(entry.path());
    }
    Ok(paths)
}

fn detect_newly_installed_directory(
    skills_path: &Path,
    before: &HashSet<PathBuf>,
) -> Result<PathBuf> {
    let mut created = Vec::new();
    for entry in std::fs::read_dir(skills_path)? {
        let entry = entry?;
        let path = entry.path();
        if !before.contains(&path) && path.is_dir() {
            created.push(path);
        }
    }

    match created.len() {
        1 => Ok(created.remove(0)),
        0 => anyhow::bail!(
            "Unable to determine installed skill directory after clone (no new directory found)"
        ),
        _ => anyhow::bail!(
            "Unable to determine installed skill directory after clone (multiple new directories found)"
        ),
    }
}

fn enforce_skill_security_audit(
    skill_path: &Path,
    allow_scripts: bool,
) -> Result<audit::SkillAuditReport> {
    let report = audit::audit_skill_directory_with_options(
        skill_path,
        audit::SkillAuditOptions { allow_scripts },
    )?;
    if report.is_clean() {
        return Ok(report);
    }

    anyhow::bail!("Skill security audit failed: {}", report.summary());
}

fn remove_git_metadata(skill_path: &Path) -> Result<()> {
    let git_dir = skill_path.join(".git");
    if git_dir.exists() {
        std::fs::remove_dir_all(&git_dir)
            .with_context(|| format!("failed to remove {}", git_dir.display()))?;
    }
    Ok(())
}

fn copy_dir_recursive_secure(src: &Path, dest: &Path) -> Result<()> {
    let src_meta = std::fs::symlink_metadata(src)
        .with_context(|| format!("failed to read metadata for {}", src.display()))?;
    if src_meta.file_type().is_symlink() {
        anyhow::bail!(
            "Refusing to copy symlinked skill source path: {}",
            src.display()
        );
    }
    if !src_meta.is_dir() {
        anyhow::bail!("Skill source must be a directory: {}", src.display());
    }

    std::fs::create_dir_all(dest)
        .with_context(|| format!("failed to create destination {}", dest.display()))?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&src_path)
            .with_context(|| format!("failed to read metadata for {}", src_path.display()))?;

        if metadata.file_type().is_symlink() {
            anyhow::bail!(
                "Refusing to copy symlink within skill source: {}",
                src_path.display()
            );
        }

        if metadata.is_dir() {
            copy_dir_recursive_secure(&src_path, &dest_path)?;
        } else if metadata.is_file() {
            std::fs::copy(&src_path, &dest_path).with_context(|| {
                format!(
                    "failed to copy skill file from {} to {}",
                    src_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn install_local_skill_source(
    source: &str,
    skills_path: &Path,
    allow_scripts: bool,
) -> Result<(PathBuf, usize)> {
    let source_path = PathBuf::from(source);
    if !source_path.exists() {
        anyhow::bail!("Source path does not exist: {source}");
    }

    let source_path = source_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize source path {source}"))?;
    let _ = enforce_skill_security_audit(&source_path, allow_scripts)?;

    let name = source_path
        .file_name()
        .context("Source path must include a directory name")?;
    let dest = skills_path.join(name);
    if dest.exists() {
        anyhow::bail!("Destination skill already exists: {}", dest.display());
    }

    if let Err(err) = copy_dir_recursive_secure(&source_path, &dest) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(err);
    }

    match enforce_skill_security_audit(&dest, allow_scripts) {
        Ok(report) => Ok((dest, report.files_scanned)),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&dest);
            Err(err)
        }
    }
}

fn install_git_skill_source(
    source: &str,
    skills_path: &Path,
    allow_scripts: bool,
) -> Result<(PathBuf, usize)> {
    let before = snapshot_skill_children(skills_path)?;
    let output = crate::util::hidden_sync_command("git")
        .args(["clone", "--depth", "1", source])
        .current_dir(skills_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Git clone failed: {stderr}");
    }

    let installed_dir = detect_newly_installed_directory(skills_path, &before)?;
    remove_git_metadata(&installed_dir)?;
    match enforce_skill_security_audit(&installed_dir, allow_scripts) {
        Ok(report) => Ok((installed_dir, report.files_scanned)),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&installed_dir);
            Err(err)
        }
    }
}

fn install_clawhub_skill_source(
    source: &str,
    skills_path: &Path,
    allow_scripts: bool,
) -> Result<(PathBuf, usize)> {
    let download_url = clawhub_download_url(source)
        .with_context(|| format!("invalid ClawhHub source: {source}"))?;
    let skill_dir_name = clawhub_skill_dir_name(source)?;
    let installed_dir = skills_path.join(&skill_dir_name);
    if installed_dir.exists() {
        anyhow::bail!(
            "Destination skill already exists: {}",
            installed_dir.display()
        );
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let resp = client
        .get(&download_url)
        .send()
        .with_context(|| format!("failed to fetch zip from {download_url}"))?;

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("ClawhHub rate limit reached (HTTP 429). Wait a moment and retry.");
    }
    if !resp.status().is_success() {
        anyhow::bail!("ClawhHub download failed (HTTP {})", resp.status());
    }

    let bytes = resp.bytes()?.to_vec();
    if bytes.len() as u64 > MAX_CLAWHUB_ZIP_BYTES {
        anyhow::bail!(
            "ClawhHub zip rejected: too large ({} bytes > {})",
            bytes.len(),
            MAX_CLAWHUB_ZIP_BYTES
        );
    }

    std::fs::create_dir_all(&installed_dir)?;

    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).context("downloaded content is not a valid zip")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let raw_name = entry.name().to_string();

        if raw_name.is_empty()
            || raw_name.contains("..")
            || raw_name.starts_with('/')
            || raw_name.contains('\\')
            || raw_name.contains(':')
        {
            let _ = std::fs::remove_dir_all(&installed_dir);
            anyhow::bail!("zip entry contains unsafe path: {raw_name}");
        }

        let out_path = installed_dir.join(&raw_name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut out_file = std::fs::File::create(&out_path)
            .with_context(|| format!("failed to create extracted file: {}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out_file)?;
    }

    let has_manifest =
        installed_dir.join("SKILL.md").exists() || installed_dir.join("SKILL.toml").exists();
    if !has_manifest {
        std::fs::write(
            installed_dir.join("SKILL.toml"),
            format!(
                "[skill]\nname = \"{}\"\ndescription = \"ClawhHub installed skill\"\nversion = \"0.1.0\"\n",
                skill_dir_name
            ),
        )?;
    }

    match enforce_skill_security_audit(&installed_dir, allow_scripts) {
        Ok(report) => Ok((installed_dir, report.files_scanned)),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&installed_dir);
            Err(err)
        }
    }
}

#[allow(clippy::too_many_lines)]
enum SkillTestOutcome {
    NoTests(String),
    Results(Vec<testing::SkillTestResult>),
}

pub async fn handle_command(
    command: crate::SkillCommands,
    config: &crate::config::Config,
) -> Result<()> {
    let workspace_dir = &config.workspace_dir;
    match command {
        crate::SkillCommands::List => {
            let skills = load_skills_with_config(workspace_dir, config);
            if skills.is_empty() {
                println!("No skills installed.");
                println!();
                println!("  Create one: mkdir -p ~/.senweavercoding/workspace/skills/my-skill");
                println!(
                    "              echo '# My Skill' > ~/.senweavercoding/workspace/skills/my-skill/SKILL.md"
                );
                println!();
                println!("  Or install: sen skills install <source>");
            } else {
                println!("Installed skills ({}):", skills.len());
                println!();
                for skill in &skills {
                    println!(
                        "  {} {}  -  {}",
                        console::style(&skill.name).white().bold(),
                        console::style(format!("v{}", skill.version)).dim(),
                        skill.description
                    );
                    if !skill.tools.is_empty() {
                        println!(
                            "    Tools: {}",
                            skill
                                .tools
                                .iter()
                                .map(|t| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    if !skill.tags.is_empty() {
                        println!("    Tags:  {}", skill.tags.join(", "));
                    }
                }
            }
            println!();
            Ok(())
        }
        crate::SkillCommands::Audit { source } => {
            let source_path = PathBuf::from(&source);
            let target = if source_path.exists() {
                source_path
            } else {
                skills_dir(workspace_dir).join(&source)
            };

            if !target.exists() {
                anyhow::bail!("Skill source or installed skill not found: {source}");
            }

            let report = audit::audit_skill_directory_with_options(
                &target,
                audit::SkillAuditOptions {
                    allow_scripts: config.skills.allow_scripts,
                },
            )?;
            if report.is_clean() {
                println!(
                    "  {} Skill audit passed for {} ({} files scanned).",
                    console::style("✓").green().bold(),
                    target.display(),
                    report.files_scanned
                );
                return Ok(());
            }

            println!(
                "  {} Skill audit failed for {}",
                console::style("✗").red().bold(),
                target.display()
            );
            for finding in report.findings {
                println!("    - {finding}");
            }
            anyhow::bail!("Skill audit failed.");
        }
        crate::SkillCommands::Install { source } => {
            println!("Installing skill from: {source}");

            let skills_path = skills_dir(workspace_dir);
            let allow_scripts = config.skills.allow_scripts;
            let (installed_dir, files_scanned) = {
                let source = source.clone();
                tokio::task::spawn_blocking(move || -> Result<(PathBuf, usize)> {
                    std::fs::create_dir_all(&skills_path)?;
                    let out = if is_clawhub_source(&source) {
                        install_clawhub_skill_source(&source, &skills_path, allow_scripts)
                            .with_context(|| {
                                format!("failed to install skill from ClawHub: {source}")
                            })?
                    } else if is_git_source(&source) {
                        install_git_skill_source(&source, &skills_path, allow_scripts)
                            .with_context(|| format!("failed to install git skill source: {source}"))?
                    } else {
                        install_local_skill_source(&source, &skills_path, allow_scripts)
                            .with_context(|| {
                                format!("failed to install local skill source: {source}")
                            })?
                    };
                    Ok(out)
                })
                .await
                .map_err(|e| anyhow::anyhow!("skill install task failed: {e}"))??
            };
            println!(
                "  {} Skill installed and audited: {} ({} files scanned)",
                console::style("✓").green().bold(),
                installed_dir.display(),
                files_scanned
            );

            println!("  Security audit completed successfully.");
            Ok(())
        }
        crate::SkillCommands::Remove { name } => {

            if name.contains("..") || name.contains('/') || name.contains('\\') {
                anyhow::bail!("Invalid skill name: {name}");
            }

            let skill_path = skills_dir(workspace_dir).join(&name);

            let canonical_skills = skills_dir(workspace_dir)
                .canonicalize()
                .unwrap_or_else(|_| skills_dir(workspace_dir));
            if let Ok(canonical_skill) = skill_path.canonicalize() {
                if !canonical_skill.starts_with(&canonical_skills) {
                    anyhow::bail!("Skill path escapes skills directory: {name}");
                }
            }

            if !skill_path.exists() {
                anyhow::bail!("Skill not found: {name}");
            }

            std::fs::remove_dir_all(&skill_path)?;
            println!(
                "  {} Skill '{}' removed.",
                console::style("✓").green().bold(),
                name
            );
            Ok(())
        }
        crate::SkillCommands::Test { name, verbose } => {
            let skills_path = skills_dir(workspace_dir);
            let outcome = tokio::task::spawn_blocking(move || -> Result<SkillTestOutcome> {
                if let Some(skill_name) = name {
                    let source_path = PathBuf::from(&skill_name);
                    let target = if source_path.exists() {
                        source_path
                    } else {
                        skills_path.join(&skill_name)
                    };

                    if !target.exists() {
                        anyhow::bail!("Skill not found: {}", skill_name);
                    }

                    let r = testing::test_skill(&target, &skill_name, verbose)?;
                    if r.tests_run == 0 {
                        return Ok(SkillTestOutcome::NoTests(skill_name));
                    }
                    Ok(SkillTestOutcome::Results(vec![r]))
                } else {
                    let dirs = vec![skills_path];
                    Ok(SkillTestOutcome::Results(testing::test_all_skills(
                        &dirs, verbose,
                    )?))
                }
            })
            .await
            .map_err(|e| anyhow::anyhow!("skill test task failed: {e}"))??;

            let results = match outcome {
                SkillTestOutcome::NoTests(skill_name) => {
                    println!(
                        "  {} No TEST.sh found for skill '{}'.",
                        console::style("-").dim(),
                        skill_name,
                    );
                    return Ok(());
                }
                SkillTestOutcome::Results(results) => results,
            };

            testing::print_results(&results);

            let any_failed = results.iter().any(|r| !r.failures.is_empty());
            if any_failed {
                anyhow::bail!("Some skill tests failed.");
            }
            Ok(())
        }
        crate::SkillCommands::Search { query } => {
            println!("Searching remote skill sources for '{query}'...");
            let forge_config = crate::skillforge::SkillForgeConfig {
                enabled: true,
                auto_integrate: false,
                ..Default::default()
            };
            let forge = crate::skillforge::SkillForge::new(forge_config);
            let report = forge.forge().await?;

            let query_lower = query.to_lowercase();
            let matching: Vec<_> = report
                .results
                .iter()
                .filter(|r| {
                    r.candidate.name.to_lowercase().contains(&query_lower)
                        || r.candidate
                            .description
                            .to_lowercase()
                            .contains(&query_lower)
                })
                .collect();

            if matching.is_empty() {
                println!("No skills found matching '{query}'.");
            } else {
                println!();
                println!("Found {} skill(s) matching '{query}':", matching.len());
                println!();
                for r in matching.iter().take(20) {
                    println!(
                        "  {} {}  -  {}",
                        console::style(&r.candidate.name).white().bold(),
                        console::style(format!("[{:?}]", r.candidate.source)).dim(),
                        r.candidate.description
                    );
                    println!(
                        "    {} (score: {:.2})",
                        console::style(&r.candidate.url).cyan(),
                        r.total_score
                    );
                }
            }
            Ok(())
        }
    }
}
