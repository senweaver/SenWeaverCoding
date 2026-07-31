// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use super::common;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use lopdf::{Document, Object, ObjectId};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct PdfOpsTool {
    security: Arc<SecurityPolicy>,
}

impl PdfOpsTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for PdfOpsTool {
    fn name(&self) -> &str {
        "pdf_ops"
    }

    fn description(&self) -> &str {
        "Native page-level PDF operations (pure Rust, no external tools). Actions: \
         `merge` (concatenate several PDFs into one), `split` (one file per page), \
         `extract` (keep only a page range into a new file), `delete_pages` (drop a page range), \
         `rotate` (rotate pages by a multiple of 90 degrees), and `info` (page count + metadata). \
         Use `pages` like `1-3,5,8-` (1-based). Outputs are written into the workspace and surfaced in the IDE. \
         To CREATE a PDF from content use `document_convert` with target_format=pdf; to READ text use `file_read`/`pdf_read`."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["merge", "split", "extract", "delete_pages", "rotate", "info"],
                    "description": "Operation to perform."
                },
                "input": { "type": "string", "description": "Source PDF path (for split/extract/delete_pages/rotate/info)." },
                "inputs": { "type": "array", "items": { "type": "string" }, "description": "Source PDF paths in order (for merge)." },
                "output": { "type": "string", "description": "Destination PDF path (for merge/extract/delete_pages/rotate)." },
                "output_dir": { "type": "string", "description": "Destination directory for split output files. Defaults to the source file's directory." },
                "pages": { "type": "string", "description": "1-based page selection, e.g. `1-3,5,8-` (extract/delete_pages; optional for rotate = all pages)." },
                "angle": { "type": "integer", "description": "Rotation in degrees, multiple of 90 (rotate)." }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        let result = match action.as_str() {
            "info" => self.run_info(args).await,
            "merge" => self.run_merge(args).await,
            "split" => self.run_split(args).await,
            "extract" => self.run_modify(args, ModifyKind::Extract).await,
            "delete_pages" => self.run_modify(args, ModifyKind::Delete).await,
            "rotate" => self.run_rotate(args).await,
            other => Err(format!(
                "Unknown action '{other}'. Expected merge/split/extract/delete_pages/rotate/info."
            )),
        };
        match result {
            Ok(output) => Ok(ToolResult {
                success: true,
                output,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e),
            }),
        }
    }
}

#[derive(Clone, Copy)]
enum ModifyKind {
    Extract,
    Delete,
}

impl PdfOpsTool {
    fn arg_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
        args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
    }

    async fn emit_written(&self, target: &std::path::Path, bytes: &[u8]) {
        let _write_guard = match crate::session::acquire_file_write_guard(target).await {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!(
                    target = %target.display(),
                    error = %e,
                    "pdf_ops: failed to acquire file write lock; skipping write"
                );
                return;
            }
        };
        let before = tokio::fs::read(target).await.ok();
        if tokio::fs::write(target, bytes).await.is_ok() {
            crate::session::record_write_for_current_session(target);
            crate::agent::file_edit_emitter::emit_file_edit(
                target,
                before.as_deref(),
                Some(bytes),
                None,
            )
            .await;
        }
    }

    async fn run_info(&self, args: serde_json::Value) -> Result<String, String> {
        let input = Self::arg_str(&args, "input").ok_or("info requires 'input'")?;
        let src = common::resolve_read_source(&self.security, input)?;
        let report = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let doc = Document::load(&src).map_err(|e| format!("failed to open PDF: {e}"))?;
            let pages = doc.get_pages().len();
            let mut lines = vec![format!("Pages: {pages}")];
            lines.push(format!("PDF version: {}", doc.version));
            if let Ok(info_ref) = doc.trailer.get(b"Info") {
                if let Ok(id) = info_ref.as_reference() {
                    if let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict()) {
                        for key in ["Title", "Author", "Subject", "Creator", "Producer"] {
                            if let Ok(v) = dict.get(key.as_bytes()) {
                                if let Ok(s) = v.as_str() {
                                    let val = String::from_utf8_lossy(s);
                                    if !val.trim().is_empty() {
                                        lines.push(format!("{key}: {val}"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(lines.join("\n"))
        })
        .await
        .map_err(|e| format!("pdf info task failed: {e}"))??;
        Ok(report)
    }

    async fn run_merge(&self, args: serde_json::Value) -> Result<String, String> {
        let inputs = args
            .get("inputs")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if inputs.len() < 2 {
            return Err("merge requires 'inputs' with at least 2 PDF paths".to_string());
        }
        let output = Self::arg_str(&args, "output").ok_or("merge requires 'output'")?;
        let mut sources = Vec::with_capacity(inputs.len());
        for p in &inputs {
            sources.push(common::resolve_read_source(&self.security, p)?);
        }
        let target = common::resolve_write_target(&self.security, output)?;
        let count = sources.len();
        let bytes = tokio::task::spawn_blocking(move || merge_documents(sources))
            .await
            .map_err(|e| format!("pdf merge task failed: {e}"))??;
        self.emit_written(&target, &bytes).await;
        Ok(format!(
            "Merged {count} PDFs into `{}` ({} bytes).",
            output,
            bytes.len()
        ))
    }

    async fn run_split(&self, args: serde_json::Value) -> Result<String, String> {
        let input = Self::arg_str(&args, "input").ok_or("split requires 'input'")?;
        let src = common::resolve_read_source(&self.security, input)?;
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "page".to_string());
        let out_dir = match Self::arg_str(&args, "output_dir") {
            Some(d) => common::resolve_write_dir(&self.security, d)?,
            None => {
                let parent = src
                    .parent()
                    .map(|p| p.to_path_buf())
                    .ok_or("cannot determine output directory")?;
                if !self.security.can_act() {
                    return Err("Action blocked: autonomy is read-only".to_string());
                }
                if !self.security.is_resolved_path_allowed(&parent)
                    || !crate::security::sandbox_allows_path(&parent)
                {
                    return Err(format!(
                        "Sandbox/policy denies writing split output to {}; pass an explicit 'output_dir'.",
                        parent.display()
                    ));
                }
                if !self.security.record_action() {
                    return Err("Rate limit exceeded: action budget exhausted".to_string());
                }
                parent
            }
        };
        let pieces = tokio::task::spawn_blocking(move || split_document(src))
            .await
            .map_err(|e| format!("pdf split task failed: {e}"))??;
        let total = pieces.len();
        let mut written = 0usize;
        for (page_no, bytes) in pieces {
            let name = format!("{stem}_p{page_no}.pdf");
            let target = out_dir.join(&name);
            self.emit_written(&target, &bytes).await;
            written += 1;
        }
        Ok(format!(
            "Split into {written}/{total} single-page PDFs under `{}`.",
            out_dir.display()
        ))
    }

    async fn run_modify(&self, args: serde_json::Value, kind: ModifyKind) -> Result<String, String> {
        let input = Self::arg_str(&args, "input").ok_or("requires 'input'")?;
        let output = Self::arg_str(&args, "output").ok_or("requires 'output'")?;
        let pages = Self::arg_str(&args, "pages")
            .ok_or("requires 'pages' (e.g. 1-3,5)")?
            .to_string();
        let src = common::resolve_read_source(&self.security, input)?;
        let target = common::resolve_write_target(&self.security, output)?;
        let (bytes, kept) = tokio::task::spawn_blocking(move || modify_document(src, &pages, kind))
            .await
            .map_err(|e| format!("pdf task failed: {e}"))??;
        self.emit_written(&target, &bytes).await;
        let verb = match kind {
            ModifyKind::Extract => "Extracted",
            ModifyKind::Delete => "Kept",
        };
        Ok(format!(
            "{verb} {kept} page(s) into `{}` ({} bytes).",
            output,
            bytes.len()
        ))
    }

    async fn run_rotate(&self, args: serde_json::Value) -> Result<String, String> {
        let input = Self::arg_str(&args, "input").ok_or("rotate requires 'input'")?;
        let output = Self::arg_str(&args, "output").ok_or("rotate requires 'output'")?;
        let angle = args
            .get("angle")
            .and_then(|v| v.as_i64())
            .ok_or("rotate requires integer 'angle'")?;
        if angle % 90 != 0 {
            return Err("angle must be a multiple of 90".to_string());
        }
        let pages = Self::arg_str(&args, "pages").map(str::to_string);
        let src = common::resolve_read_source(&self.security, input)?;
        let target = common::resolve_write_target(&self.security, output)?;
        let bytes = tokio::task::spawn_blocking(move || rotate_document(src, angle, pages))
            .await
            .map_err(|e| format!("pdf rotate task failed: {e}"))??;
        self.emit_written(&target, &bytes).await;
        Ok(format!(
            "Rotated pages by {angle} degrees into `{}` ({} bytes).",
            output,
            bytes.len()
        ))
    }
}

fn serialize(mut doc: Document) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    doc.save_to(&mut buf)
        .map_err(|e| format!("failed to serialize PDF: {e}"))?;
    Ok(buf)
}

fn rotate_document(src: PathBuf, angle: i64, pages: Option<String>) -> Result<Vec<u8>, String> {
    let mut doc = Document::load(&src).map_err(|e| format!("failed to open PDF: {e}"))?;
    let page_map = doc.get_pages();
    let total = page_map.len();
    let selected: Option<std::collections::HashSet<u32>> = match pages {
        Some(spec) => Some(common::parse_page_ranges(&spec, total)?.into_iter().collect()),
        None => None,
    };
    for (page_no, page_id) in page_map {
        if let Some(ref sel) = selected {
            if !sel.contains(&page_no) {
                continue;
            }
        }
        if let Ok(dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
            let current = dict.get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0);
            dict.set("Rotate", (current + angle).rem_euclid(360));
        }
    }
    serialize(doc)
}

fn modify_document(src: PathBuf, pages: &str, kind: ModifyKind) -> Result<(Vec<u8>, usize), String> {
    let mut doc = Document::load(&src).map_err(|e| format!("failed to open PDF: {e}"))?;
    let page_map = doc.get_pages();
    let total = page_map.len();
    let selected: std::collections::HashSet<u32> =
        common::parse_page_ranges(pages, total)?.into_iter().collect();
    let to_delete: Vec<u32> = match kind {
        ModifyKind::Delete => selected.iter().copied().collect(),
        ModifyKind::Extract => page_map
            .keys()
            .copied()
            .filter(|n| !selected.contains(n))
            .collect(),
    };
    let kept = total.saturating_sub(to_delete.len());
    if kept == 0 {
        return Err("operation would remove every page".to_string());
    }
    doc.delete_pages(&to_delete);
    Ok((serialize(doc)?, kept))
}

fn split_document(src: PathBuf) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let probe = Document::load(&src).map_err(|e| format!("failed to open PDF: {e}"))?;
    let page_numbers: Vec<u32> = probe.get_pages().keys().copied().collect();
    drop(probe);
    const MAX_PAGES: usize = 500;
    if page_numbers.len() > MAX_PAGES {
        return Err(format!(
            "refusing to split a {}-page PDF (limit {MAX_PAGES}); use extract with a page range instead",
            page_numbers.len()
        ));
    }
    let mut out = Vec::with_capacity(page_numbers.len());
    for page_no in page_numbers {
        let mut doc = Document::load(&src).map_err(|e| format!("failed to open PDF: {e}"))?;
        let others: Vec<u32> = doc
            .get_pages()
            .keys()
            .copied()
            .filter(|n| *n != page_no)
            .collect();
        doc.delete_pages(&others);
        out.push((page_no, serialize(doc)?));
    }
    Ok(out)
}

fn merge_documents(sources: Vec<PathBuf>) -> Result<Vec<u8>, String> {
    let mut max_id = 1u32;
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut document = Document::with_version("1.5");

    for path in sources {
        let mut doc = Document::load(&path)
            .map_err(|e| format!("failed to open `{}`: {e}", path.display()))?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        let collected: Vec<(ObjectId, Object)> = doc
            .get_pages()
            .into_values()
            .filter_map(|object_id| doc.get_object(object_id).ok().map(|o| (object_id, o.to_owned())))
            .collect();
        for (k, v) in collected {
            documents_pages.insert(k, v);
        }
        documents_objects.extend(doc.objects);
    }

    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    for (object_id, object) in documents_objects.into_iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog_object = Some((
                    catalog_object.map(|(id, _)| id).unwrap_or(object_id),
                    object,
                ));
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref old)) = pages_object {
                        if let Ok(old_dict) = old.as_dict() {
                            dictionary.extend(old_dict);
                        }
                    }
                    pages_object = Some((
                        pages_object.map(|(id, _)| id).unwrap_or(object_id),
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                document.objects.insert(object_id, object);
            }
        }
    }

    let Some((pages_id, pages_obj)) = pages_object else {
        return Err("no Pages root found in inputs".to_string());
    };
    let Some((catalog_id, catalog_obj)) = catalog_object else {
        return Err("no Catalog root found in inputs".to_string());
    };

    for (object_id, object) in documents_pages.iter() {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_id);
            document
                .objects
                .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    if let Ok(dictionary) = pages_obj.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", documents_pages.len() as u32);
        dictionary.set(
            "Kids",
            documents_pages
                .keys()
                .map(|object_id| Object::Reference(*object_id))
                .collect::<Vec<_>>(),
        );
        document.objects.insert(pages_id, Object::Dictionary(dictionary));
    }

    if let Ok(dictionary) = catalog_obj.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_id);
        dictionary.remove(b"Outlines");
        document.objects.insert(catalog_id, Object::Dictionary(dictionary));
    }

    document.trailer.set("Root", catalog_id);
    document.max_id = document.objects.len() as u32;
    document.renumber_objects();
    document.adjust_zero_pages();

    serialize(document)
}
