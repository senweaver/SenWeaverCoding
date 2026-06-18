// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use include_dir::{include_dir, Dir};
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::OnceLock;

static PROMPT_TEMPLATES_DIR: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/assets/prompt-templates");

pub const SURFACES: &[&str] = &["image", "video"];

#[derive(Debug, Clone, Serialize)]
pub struct PromptTemplateMeta {
    pub id: String,
    pub surface: String,
    pub title: String,
    pub summary: String,
    pub category: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "previewImageUrl")]
    pub preview_image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "previewVideoUrl")]
    pub preview_video_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptTemplateDetail {
    pub meta: PromptTemplateMeta,
    pub prompt: String,
}

fn is_surface(surface: &str) -> bool {
    SURFACES.contains(&surface)
}

fn str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse(raw: &str, surface: &str, id: &str) -> Option<PromptTemplateDetail> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let title = str_field(&v, "title")?;
    let prompt = str_field(&v, "prompt")?;
    if prompt.len() < 20 {
        return None;
    }
    let tags = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let meta = PromptTemplateMeta {
        id: id.to_string(),
        surface: surface.to_string(),
        title,
        summary: str_field(&v, "summary").unwrap_or_default(),
        category: str_field(&v, "category").unwrap_or_else(|| "General".to_string()),
        tags,
        model: str_field(&v, "model"),
        aspect: str_field(&v, "aspect"),
        preview_image_url: str_field(&v, "previewImageUrl"),
        preview_video_url: str_field(&v, "previewVideoUrl"),
    };
    Some(PromptTemplateDetail { meta, prompt })
}

fn build_catalog() -> Vec<PromptTemplateMeta> {
    let mut out: Vec<PromptTemplateMeta> = Vec::new();
    for surface in SURFACES {
        let Some(dir) = PROMPT_TEMPLATES_DIR.get_dir(surface) else {
            continue;
        };
        for file in dir.files() {
            let Some(name) = file.path().file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            let Some(raw) = file.contents_utf8() else {
                continue;
            };
            if let Some(detail) = parse(raw, surface, id) {
                out.push(detail.meta);
            }
        }
    }
    out.sort_by(|a, b| {
        a.surface
            .cmp(&b.surface)
            .then_with(|| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()))
    });
    out
}

pub fn catalog() -> &'static [PromptTemplateMeta] {
    static CACHE: OnceLock<Vec<PromptTemplateMeta>> = OnceLock::new();
    CACHE.get_or_init(build_catalog).as_slice()
}

pub fn read(surface: &str, id: &str) -> Option<PromptTemplateDetail> {
    if !is_surface(surface) {
        return None;
    }
    let id = id.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return None;
    }
    let raw = PROMPT_TEMPLATES_DIR
        .get_file(format!("{surface}/{id}.json"))
        .and_then(|f| f.contents_utf8())?;
    parse(raw, surface, id)
}

pub fn title_for(surface: &str, id: &str) -> Option<String> {
    catalog()
        .iter()
        .find(|m| m.surface == surface && m.id == id)
        .map(|m| m.title.clone())
}

pub fn read_raw(surface: &str, id: &str) -> Option<&'static str> {
    if !is_surface(surface) {
        return None;
    }
    let id = id.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return None;
    }
    PROMPT_TEMPLATES_DIR
        .get_file(format!("{surface}/{id}.json"))
        .and_then(|f| f.contents_utf8())
}

fn library_store() -> Option<&'static crate::services::TemplateLibraryStore> {
    crate::services::try_get_services().map(|s| &s.template_library)
}

pub fn resolved_read(surface: &str, id: &str) -> Option<PromptTemplateDetail> {
    if !is_surface(surface) {
        return None;
    }
    let id = id.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return None;
    }
    if let Some(store) = library_store() {
        if let Some(raw) = store.read(&format!("prompt-templates/{surface}/{id}.json")) {
            if let Some(detail) = parse(&raw, surface, id) {
                return Some(detail);
            }
        }
    }
    read(surface, id)
}

pub fn resolved_catalog() -> Vec<PromptTemplateMeta> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out: Vec<PromptTemplateMeta> = Vec::new();
    for m in catalog() {
        seen.insert((m.surface.clone(), m.id.clone()));
        let mut meta = m.clone();
        if let Some(store) = library_store() {
            if let Some(raw) =
                store.read(&format!("prompt-templates/{}/{}.json", m.surface, m.id))
            {
                if let Some(detail) = parse(&raw, &m.surface, &m.id) {
                    meta = detail.meta;
                }
            }
        }
        out.push(meta);
    }
    if let Some(store) = library_store() {
        let mut extra: Vec<PromptTemplateMeta> = Vec::new();
        for surface in SURFACES {
            for f in store.list_files(&format!("prompt-templates/{surface}")) {
                let Some(fname) = f.rsplit('/').next() else {
                    continue;
                };
                let Some(id) = fname.strip_suffix(".json") else {
                    continue;
                };
                if seen.contains(&(surface.to_string(), id.to_string())) {
                    continue;
                }
                let Some(raw) = store.read(&f) else {
                    continue;
                };
                if let Some(detail) = parse(&raw, surface, id) {
                    extra.push(detail.meta);
                }
            }
        }
        extra.sort_by(|a, b| {
            a.surface
                .cmp(&b.surface)
                .then_with(|| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()))
        });
        out.extend(extra);
    }
    out
}
