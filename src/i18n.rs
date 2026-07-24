// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct ToolDescriptions {

    locale_descriptions: HashMap<String, String>,

    english_fallback: HashMap<String, String>,

    locale: String,
}

#[derive(Debug, serde::Deserialize)]
struct DescriptionFile {
    #[serde(default)]
    tools: HashMap<String, String>,
}

struct DescCacheEntry {
    descs: ToolDescriptions,
    cached_at: Instant,
}

const DESC_CACHE_TTL: Duration = Duration::from_secs(60);
const DESC_CACHE_MAX_ENTRIES: usize = 32;

fn desc_cache() -> &'static Mutex<HashMap<String, DescCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, DescCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

impl ToolDescriptions {

    pub fn load(locale: &str, search_dirs: &[PathBuf]) -> Self {
        let key = {
            let mut k = String::from(locale);
            for dir in search_dirs {
                k.push('\u{1f}');
                k.push_str(&dir.display().to_string());
            }
            k
        };
        if let Ok(guard) = desc_cache().lock() {
            if let Some(entry) = guard.get(&key) {
                if entry.cached_at.elapsed() < DESC_CACHE_TTL {
                    return entry.descs.clone();
                }
            }
        }
        let built = Self::load_uncached(locale, search_dirs);
        if let Ok(mut guard) = desc_cache().lock() {
            if guard.len() >= DESC_CACHE_MAX_ENTRIES {
                guard.clear();
            }
            guard.insert(
                key,
                DescCacheEntry {
                    descs: built.clone(),
                    cached_at: Instant::now(),
                },
            );
        }
        built
    }

    fn load_uncached(locale: &str, search_dirs: &[PathBuf]) -> Self {
        let mut locale_descriptions = load_locale_file(locale, search_dirs);

        let mut english_fallback = if locale == "en" {
            HashMap::new()
        } else {
            load_locale_file("en", search_dirs)
        };

        debug!(
            locale = locale,
            locale_keys = locale_descriptions.len(),
            english_keys = english_fallback.len(),
            "tool descriptions loaded"
        );

        merge_tier_fallbacks(&mut locale_descriptions);
        if locale != "en" {
            merge_tier_fallbacks(&mut english_fallback);
        }

        Self {
            locale_descriptions,
            english_fallback,
            locale: locale.to_string(),
        }
    }

    pub fn get(&self, tool_name: &str) -> Option<&str> {
        self.locale_descriptions
            .get(tool_name)
            .or_else(|| self.english_fallback.get(tool_name))
            .map(String::as_str)
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub fn empty() -> Self {
        Self {
            locale_descriptions: HashMap::new(),
            english_fallback: HashMap::new(),
            locale: "en".to_string(),
        }
    }
}

pub fn detect_locale() -> String {
    if let Some(val) = crate::util::get_runtime_var("SEN_LOCALE") {
        let val = val.trim().to_string();
        if !val.is_empty() {
            return normalize_locale(&val);
        }
    }
    for var in &["LANG", "LC_ALL"] {
        if let Ok(val) = std::env::var(var) {
            let locale = normalize_locale(&val);
            if locale != "C" && locale != "POSIX" && !locale.is_empty() {
                return locale;
            }
        }
    }
    "en".to_string()
}

fn normalize_locale(raw: &str) -> String {

    let base = raw.split('.').next().unwrap_or(raw);

    base.replace('_', "-")
}

pub fn normalize_locale_public(raw: &str) -> String {
    normalize_locale(raw)
}

pub fn default_search_dirs(workspace_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !dirs.contains(&manifest_dir) {
        dirs.push(manifest_dir);
    }

    let trust_workspace = std::env::var("SEN_TRUST_WORKSPACE_TOOL_DESCRIPTIONS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if trust_workspace {
        let ws = workspace_dir.to_path_buf();
        if !dirs.contains(&ws) {
            dirs.insert(0, ws);
        }
    }

    dirs
}

fn locale_file_candidates(locale: &str) -> Vec<String> {
    let mut candidates = vec![
        format!("src/tool_descriptions/{locale}.toml"),
        format!("tool_descriptions/{locale}.toml"),
    ];
    if locale == "zh" || locale.starts_with("zh-") && locale != "zh-CN" {
        candidates.insert(0, "src/tool_descriptions/zh-CN.toml".to_string());
        candidates.push("tool_descriptions/zh-CN.toml".to_string());
    }
    candidates
}

fn load_locale_file(locale: &str, search_dirs: &[PathBuf]) -> HashMap<String, String> {
    for dir in search_dirs {
        for filename in locale_file_candidates(locale) {
            let path = dir.join(&filename);
            if let Ok(contents) = std::fs::read_to_string(&path) {
                match toml::from_str::<DescriptionFile>(&contents) {
                    Ok(parsed) => {
                        debug!(path = %path.display(), keys = parsed.tools.len(), "loaded locale file");
                        return parsed.tools;
                    }
                    Err(e) => {
                        debug!(path = %path.display(), error = %e, "failed to parse locale file");
                    }
                }
            }
        }
    }

    debug!(
        locale = locale,
        "no locale file found on filesystem; using embedded fallback"
    );
    embedded_locale_fallback(locale)
}

fn embedded_locale_fallback(locale: &str) -> HashMap<String, String> {
    const EN: &str = include_str!("tool_descriptions/en.toml");
    const ZH_CN: &str = include_str!("tool_descriptions/zh-CN.toml");
    let raw = if locale == "en" {
        EN
    } else if locale == "zh" || locale == "zh-CN" || locale.starts_with("zh-") {
        ZH_CN
    } else {
        EN
    };
    match toml::from_str::<DescriptionFile>(raw) {
        Ok(parsed) => parsed.tools,
        Err(e) => {
            debug!(error = %e, "failed to parse embedded locale fallback");
            HashMap::new()
        }
    }
}

fn merge_tier_fallbacks(map: &mut HashMap<String, String>) {
    for (name, entry) in crate::tools::handler::tier::TOOL_TIERS.iter() {
        map.entry((*name).to_string())
            .or_insert_with(|| entry.description.to_string());
    }
}
