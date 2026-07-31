// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use super::common;
use crate::agent::designer::deck::render::{
    RenderBackground, RenderBlock, RenderDeck, RenderFonts, RenderImageBlock, RenderParagraph,
    RenderRun, RenderSlide, RenderTableBlock, RenderTextBlock,
};
use crate::agent::designer::deck::pptx::write_render_pptx;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const STAGE_W: u32 = 1280;
const STAGE_H: u32 = 720;
const MARGIN: f64 = 80.0;

pub struct PresentationCreateTool {
    security: Arc<SecurityPolicy>,
}

impl PresentationCreateTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for PresentationCreateTool {
    fn name(&self) -> &str {
        "presentation_create"
    }

    fn description(&self) -> &str {
        "Create a real PowerPoint (.pptx) from a simple slide outline (pure Rust OOXML, no external tools). \
         Each slide picks a `layout`: `title` (cover), `section` (divider), `bullets` (title + bullet list), \
         `two_col` (title + two bullet columns), `table` (title + data table), or `image` (title + picture). \
         Provide `title`, optional `subtitle`, `bullets`/`left`/`right` (string arrays), `table` ({columns, rows}), \
         `image` (workspace path), and `notes` (speaker notes) per slide. Set deck `theme`/`accent` for colors. \
         The .pptx is written into the workspace and surfaced in the IDE."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "output_path": { "type": "string", "description": "Destination .pptx path (workspace-relative)." },
                "title": { "type": "string", "description": "Deck title (metadata)." },
                "accent": { "type": "string", "description": "Accent color hex (e.g. #4472C4). Default #4472C4." },
                "theme": { "type": "string", "description": "Optional theme label (informational)." },
                "slides": {
                    "type": "array",
                    "description": "Ordered slides.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "layout": { "type": "string", "enum": ["title", "section", "bullets", "two_col", "table", "image"] },
                            "title": { "type": "string" },
                            "subtitle": { "type": "string" },
                            "bullets": { "type": "array", "items": { "type": "string" } },
                            "left": { "type": "array", "items": { "type": "string" } },
                            "right": { "type": "array", "items": { "type": "string" } },
                            "table": {
                                "type": "object",
                                "properties": {
                                    "columns": { "type": "array", "items": { "type": "string" } },
                                    "rows": { "type": "array", "items": { "type": "array", "items": {} } }
                                }
                            },
                            "image": { "type": "string", "description": "Workspace-relative image path." },
                            "notes": { "type": "string", "description": "Speaker notes." }
                        }
                    }
                }
            },
            "required": ["output_path", "slides"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'output_path' parameter"))?
            .to_string();
        let slides_json = args
            .get("slides")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or_else(|| anyhow::anyhow!("'slides' must be a non-empty array"))?;

        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Presentation")
            .to_string();
        let accent = normalize_hex(
            args.get("accent").and_then(|v| v.as_str()).unwrap_or("#4472C4"),
        );
        let theme = args
            .get("theme")
            .and_then(|v| v.as_str())
            .unwrap_or("custom")
            .to_string();

        let mut owned_slides: Vec<serde_json::Value> = slides_json.clone();
        let mut dropped_images = 0usize;
        for s in owned_slides.iter_mut() {
            let img = s
                .get("image")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string);
            if let Some(img) = img {
                let resolved = common::resolve_read_source(&self.security, &img);
                if let Some(obj) = s.as_object_mut() {
                    match resolved {
                        Ok(abs) => {
                            obj.insert(
                                "image".to_string(),
                                serde_json::Value::String(abs.to_string_lossy().to_string()),
                            );
                        }
                        Err(_) => {
                            obj.remove("image");
                            dropped_images += 1;
                        }
                    }
                }
            }
        }

        let slides: Vec<RenderSlide> = owned_slides
            .iter()
            .enumerate()
            .map(|(idx, s)| build_slide(idx, s, &accent))
            .collect();
        let slide_count = slides.len();

        let deck = RenderDeck {
            version: 1,
            title,
            theme,
            stage_w: STAGE_W,
            stage_h: STAGE_H,
            transition: "none".to_string(),
            accent,
            fonts: default_fonts(),
            slides,
        };

        let target = match common::resolve_write_target(&self.security, &output_path) {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e),
                });
            }
        };

        let _write_guard = match crate::session::acquire_file_write_guard(&target).await {
            Ok(guard) => guard,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("{e}")),
                });
            }
        };

        let workspace = self.security.workspace_dir();
        let before = tokio::fs::read(&target).await.ok();
        let target_for_task = target.clone();
        let render = tokio::task::spawn_blocking(move || {
            write_render_pptx(&target_for_task, &deck, &workspace)
        })
        .await
        .map_err(|e| anyhow::anyhow!("pptx render task failed: {e}"))?;
        if let Err(e) = render {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("failed to write pptx: {e}")),
            });
        }

        let after = tokio::fs::read(&target).await.unwrap_or_default();
        crate::session::record_write_for_current_session(&target);
        crate::agent::file_edit_emitter::emit_file_edit(&target, before.as_deref(), Some(&after), None)
            .await;

        let note = if dropped_images > 0 {
            format!(
                " Note: {dropped_images} image(s) were skipped (path not found or not allowed by policy)."
            )
        } else {
            String::new()
        };
        Ok(ToolResult {
            success: true,
            output: format!(
                "Wrote {slide_count} slide(s) to `{output_path}` ({} bytes).{note}",
                after.len()
            ),
            error: None,
        })
    }
}

fn default_fonts() -> RenderFonts {
    RenderFonts {
        heading_latin: "Calibri".to_string(),
        heading_ea: "Microsoft YaHei".to_string(),
        body_latin: "Calibri".to_string(),
        body_ea: "Microsoft YaHei".to_string(),
        heading_css: "Calibri, 'Microsoft YaHei', sans-serif".to_string(),
        body_css: "Calibri, 'Microsoft YaHei', sans-serif".to_string(),
    }
}

fn normalize_hex(raw: &str) -> String {
    let cleaned: String = raw.trim().trim_start_matches('#').chars().take(6).collect();
    if cleaned.len() == 6 && cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("#{}", cleaned.to_uppercase())
    } else {
        "#4472C4".to_string()
    }
}

fn str_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn run(text: &str, size: f64, color: &str, bold: bool) -> RenderRun {
    RenderRun {
        text: text.to_string(),
        bold,
        italic: false,
        color: color.to_string(),
        size,
        font: "Calibri".to_string(),
    }
}

fn para(run: RenderRun, bullet: bool) -> RenderParagraph {
    RenderParagraph {
        bullet,
        level: 0,
        bullet_char: None,
        space_before: if bullet { 6.0 } else { 0.0 },
        runs: vec![run],
    }
}

fn text_block(
    id: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    align: &str,
    valign: &str,
    paragraphs: Vec<RenderParagraph>,
) -> RenderBlock {
    RenderBlock::Text(RenderTextBlock {
        id: id.to_string(),
        x,
        y,
        w,
        h,
        align: align.to_string(),
        valign: valign.to_string(),
        line_spacing: 1.15,
        paragraphs,
    })
}

fn bullet_paragraphs(items: &[String], color: &str) -> Vec<RenderParagraph> {
    items
        .iter()
        .map(|t| para(run(t, 20.0, color, false), true))
        .collect()
}

fn build_slide(idx: usize, s: &serde_json::Value, accent: &str) -> RenderSlide {
    let layout = s
        .get("layout")
        .and_then(|v| v.as_str())
        .unwrap_or("bullets")
        .to_string();
    let title = s.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let subtitle = s.get("subtitle").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let notes = s
        .get("notes")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);

    let dark = "#1A1A1A";
    let muted = "#5A5A6A";
    let content_w = STAGE_W as f64 - 2.0 * MARGIN;
    let mut blocks: Vec<RenderBlock> = Vec::new();

    match layout.as_str() {
        "title" | "section" => {
            let centered = layout == "title" || layout == "section";
            let align = if centered { "center" } else { "left" };
            blocks.push(text_block(
                "title",
                MARGIN,
                STAGE_H as f64 * 0.36,
                content_w,
                120.0,
                align,
                "middle",
                vec![para(run(&title, 44.0, accent, true), false)],
            ));
            if !subtitle.is_empty() {
                blocks.push(text_block(
                    "subtitle",
                    MARGIN,
                    STAGE_H as f64 * 0.52,
                    content_w,
                    80.0,
                    align,
                    "top",
                    vec![para(run(&subtitle, 22.0, muted, false), false)],
                ));
            }
        }
        "two_col" => {
            push_title(&mut blocks, &title, accent, content_w);
            let left = str_array(s.get("left"));
            let right = str_array(s.get("right"));
            let col_w = (content_w - 40.0) / 2.0;
            blocks.push(text_block(
                "left",
                MARGIN,
                190.0,
                col_w,
                STAGE_H as f64 - 190.0 - MARGIN,
                "left",
                "top",
                bullet_paragraphs(&left, dark),
            ));
            blocks.push(text_block(
                "right",
                MARGIN + col_w + 40.0,
                190.0,
                col_w,
                STAGE_H as f64 - 190.0 - MARGIN,
                "left",
                "top",
                bullet_paragraphs(&right, dark),
            ));
        }
        "table" => {
            push_title(&mut blocks, &title, accent, content_w);
            if let Some(tbl) = build_table_block(s, accent) {
                blocks.push(tbl);
            }
        }
        "image" => {
            push_title(&mut blocks, &title, accent, content_w);
            if let Some(src) = s.get("image").and_then(|v| v.as_str()).map(str::trim).filter(|x| !x.is_empty()) {
                blocks.push(RenderBlock::Image(RenderImageBlock {
                    id: "image".to_string(),
                    x: MARGIN,
                    y: 190.0,
                    w: content_w,
                    h: STAGE_H as f64 - 190.0 - MARGIN,
                    src: src.to_string(),
                    fit: "contain".to_string(),
                    radius: 8.0,
                }));
            }
        }
        _ => {
            push_title(&mut blocks, &title, accent, content_w);
            let bullets = str_array(s.get("bullets"));
            blocks.push(text_block(
                "body",
                MARGIN,
                190.0,
                content_w,
                STAGE_H as f64 - 190.0 - MARGIN,
                "left",
                "top",
                bullet_paragraphs(&bullets, dark),
            ));
        }
    }

    RenderSlide {
        id: format!("slide-{}", idx + 1),
        layout,
        background: RenderBackground::Color {
            color: "#FFFFFF".to_string(),
        },
        notes,
        blocks,
    }
}

fn push_title(blocks: &mut Vec<RenderBlock>, title: &str, accent: &str, content_w: f64) {
    blocks.push(text_block(
        "title",
        MARGIN,
        60.0,
        content_w,
        90.0,
        "left",
        "top",
        vec![para(run(title, 30.0, accent, true), false)],
    ));
}

fn build_table_block(s: &serde_json::Value, accent: &str) -> Option<RenderBlock> {
    let tbl = s.get("table")?;
    let columns = str_array(tbl.get("columns"));
    let rows_json = tbl.get("rows").and_then(|v| v.as_array())?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    if !columns.is_empty() {
        rows.push(columns.clone());
    }
    for r in rows_json {
        if let Some(cells) = r.as_array() {
            rows.push(cells.iter().map(value_to_text).collect());
        }
    }
    if rows.is_empty() {
        return None;
    }
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
    let col_fracs = vec![1.0 / ncols as f64; ncols];
    Some(RenderBlock::Table(RenderTableBlock {
        id: "table".to_string(),
        x: MARGIN,
        y: 190.0,
        w: STAGE_W as f64 - 2.0 * MARGIN,
        h: STAGE_H as f64 - 190.0 - MARGIN,
        col_fracs,
        header_row: !columns.is_empty(),
        rows,
        size: 16.0,
        text_color: "#1A1A1A".to_string(),
        header_fill: accent.to_string(),
        header_text: "#FFFFFF".to_string(),
        row_fill: "#F6F6F8".to_string(),
        hairline: "#C0C0C8".to_string(),
        font_css: "Calibri, 'Microsoft YaHei', sans-serif".to_string(),
        font_latin: "Calibri".to_string(),
        font_ea: "Microsoft YaHei".to_string(),
    }))
}

fn value_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}
