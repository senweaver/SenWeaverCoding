// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use include_dir::{include_dir, Dir};
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::OnceLock;

static HTML_TEMPLATES_DIR: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/assets/designer-templates");

const TEMPLATE_FILE: &str = "template.html";

#[derive(Debug, Clone, Serialize)]
pub struct HtmlTemplateMeta {
    pub id: String,
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub summary: String,
}

fn str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_manifest(raw: &str, dir_id: &str) -> Option<HtmlTemplateMeta> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let id = str_field(&v, "id").unwrap_or_else(|| dir_id.to_string());
    let title = str_field(&v, "title")?;
    let tags = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(HtmlTemplateMeta {
        id,
        title,
        category: str_field(&v, "category").unwrap_or_else(|| "General".to_string()),
        tags,
        summary: str_field(&v, "summary").unwrap_or_default(),
    })
}

fn build_catalog() -> Vec<HtmlTemplateMeta> {
    let mut out: Vec<HtmlTemplateMeta> = Vec::new();
    for dir in HTML_TEMPLATES_DIR.dirs() {
        let Some(dir_id) = dir.path().file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(manifest) = dir
            .get_file(format!("{dir_id}/manifest.json"))
            .and_then(|f| f.contents_utf8())
        else {
            continue;
        };
        if dir
            .get_file(format!("{dir_id}/{TEMPLATE_FILE}"))
            .is_none()
        {
            continue;
        }
        if let Some(meta) = parse_manifest(manifest, dir_id) {
            out.push(meta);
        }
    }
    out.sort_by(|a, b| {
        a.category
            .to_ascii_lowercase()
            .cmp(&b.category.to_ascii_lowercase())
            .then_with(|| a.title.to_ascii_lowercase().cmp(&b.title.to_ascii_lowercase()))
    });
    out
}

pub fn catalog() -> &'static [HtmlTemplateMeta] {
    static CACHE: OnceLock<Vec<HtmlTemplateMeta>> = OnceLock::new();
    CACHE.get_or_init(build_catalog).as_slice()
}

pub fn is_known(id: &str) -> bool {
    catalog().iter().any(|m| m.id == id)
}

pub fn meta_for(id: &str) -> Option<&'static HtmlTemplateMeta> {
    catalog().iter().find(|m| m.id == id)
}

pub fn title_for(id: &str) -> Option<String> {
    meta_for(id).map(|m| m.title.clone())
}

pub fn read(id: &str) -> Option<&'static str> {
    let id = id.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return None;
    }
    if !is_known(id) {
        return None;
    }
    HTML_TEMPLATES_DIR
        .get_file(format!("{id}/{TEMPLATE_FILE}"))
        .and_then(|f| f.contents_utf8())
}

pub fn read_member(id: &str, file: &str) -> Option<&'static str> {
    let id = id.trim();
    let file = file.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return None;
    }
    if file.is_empty() || file.contains("..") || file.contains('\\') || file.starts_with('/') {
        return None;
    }
    if !is_known(id) {
        return None;
    }
    HTML_TEMPLATES_DIR
        .get_file(format!("{id}/{file}"))
        .and_then(|f| f.contents_utf8())
}

pub fn injection(id: &str) -> Option<String> {
    let meta = resolved_meta(id)?;
    let mut out = format!(
        "\n\n## Built-in starting template — {title} ({id})\n\n\
         The user picked this curated, self-contained HTML template as the starting point. \
         Before writing, read its full markup with the `designer_template_read` tool \
         (`id` = `{id}`), then write that markup to your artifact file as the foundation and \
         adapt it to the brief — keep its layout structure, grid, type scale, and component \
         rhythm; replace only copy, data, brand tokens, and imagery. Do not regress its craft.",
        title = meta.title,
        id = meta.id,
    );
    if !meta.summary.is_empty() {
        out.push_str(&format!("\nTemplate summary: {}", meta.summary));
    }
    if !meta.tags.is_empty() {
        out.push_str(&format!("\nTemplate tags: {}", meta.tags.join(", ")));
    }
    Some(out)
}

fn library_store() -> Option<&'static crate::services::TemplateLibraryStore> {
    crate::services::try_get_services().map(|s| &s.template_library)
}

pub fn resolved_is_known(id: &str) -> bool {
    if is_known(id) {
        return true;
    }
    library_store()
        .map(|s| s.exists(&format!("designer-templates/{id}/{TEMPLATE_FILE}")))
        .unwrap_or(false)
}

fn resolved_meta(id: &str) -> Option<HtmlTemplateMeta> {
    if let Some(store) = library_store() {
        if let Some(raw) = store.read(&format!("designer-templates/{id}/manifest.json")) {
            if let Some(meta) = parse_manifest(&raw, id) {
                return Some(meta);
            }
        }
    }
    meta_for(id).cloned()
}

pub fn resolved_read(id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
        return None;
    }
    if !resolved_is_known(id) {
        return None;
    }
    if let Some(store) = library_store() {
        if let Some(content) = store.read(&format!("designer-templates/{id}/{TEMPLATE_FILE}")) {
            return Some(content);
        }
    }
    read(id).map(str::to_string)
}

pub fn resolved_catalog() -> Vec<HtmlTemplateMeta> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<HtmlTemplateMeta> = Vec::new();
    for m in catalog() {
        seen.insert(m.id.clone());
        out.push(resolved_meta(&m.id).unwrap_or_else(|| m.clone()));
    }
    if let Some(store) = library_store() {
        let mut extra: Vec<String> = store
            .child_dirs("designer-templates")
            .into_iter()
            .filter(|id| {
                !seen.contains(id)
                    && store.exists(&format!("designer-templates/{id}/{TEMPLATE_FILE}"))
                    && store.exists(&format!("designer-templates/{id}/manifest.json"))
            })
            .collect();
        extra.sort();
        for id in extra {
            if let Some(meta) = resolved_meta(&id) {
                out.push(meta);
            }
        }
    }
    out
}
