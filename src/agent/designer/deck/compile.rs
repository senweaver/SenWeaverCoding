// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::render::{
    RenderBackground, RenderBlock, RenderDeck, RenderFonts, RenderImageBlock, RenderPaint,
    RenderParagraph, RenderRun, RenderShapeBlock, RenderSlide, RenderStroke, RenderTableBlock,
    RenderTextBlock,
};
use super::spec::{
    stage_for_aspect, BlockSpec, BulletItemSpec, DeckManifest, SlideSpec,
};
use super::theme::{
    self, role_preset, DeckTheme, FontKind, LayoutSlot,
};

pub const MANIFEST_FILE: &str = "deck.json";
pub const SLIDES_DIR: &str = "slides";
pub const RENDER_FILE: &str = "render.json";
pub const PPTX_FILE: &str = "deck.pptx";

#[derive(Debug, Clone)]
pub struct CompileFinding {
    pub severity: &'static str,
    pub location: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct CompileOutcome {
    pub findings: Vec<CompileFinding>,
    pub slide_count: usize,
    pub pending_slides: Vec<String>,
    pub wrote_outputs: bool,
    pub pptx_path: Option<PathBuf>,
}

impl CompileOutcome {
    fn push(&mut self, severity: &'static str, location: impl Into<String>, message: impl Into<String>) {
        self.findings.push(CompileFinding {
            severity,
            location: location.into(),
            message: message.into(),
        });
    }

    pub fn count(&self, severity: &str) -> usize {
        self.findings.iter().filter(|f| f.severity == severity).count()
    }

    pub fn format_report(&self, deck_rel: &str) -> String {
        let p0 = self.count("P0");
        let p1 = self.count("P1");
        let p2 = self.count("P2");
        let mut out = format!(
            "Deck compile — {deck_rel}\nSlides compiled: {} · P0: {p0} · P1: {p1} · P2: {p2}\n",
            self.slide_count
        );
        if self.wrote_outputs {
            if let Some(p) = &self.pptx_path {
                out.push_str(&format!(
                    "Outputs written: {RENDER_FILE} (canvas preview) and `{}` (final PPTX file).\n",
                    p.display()
                ));
            }
        } else {
            out.push_str("Outputs NOT written — fix the P0 findings and re-run.\n");
        }
        if !self.pending_slides.is_empty() {
            out.push_str(&format!(
                "Slides listed in deck.json but not written yet: {}.\n",
                self.pending_slides.join(", ")
            ));
        }
        for f in &self.findings {
            out.push_str(&format!("\n[{}] {}: {}", f.severity, f.location, f.message));
        }
        if p0 > 0 {
            out.push_str(
                "\n\nFix every P0 finding (and write every pending slide file), then call `deck_compile` again until the deck is clean.",
            );
        } else if self.findings.is_empty() && self.pending_slides.is_empty() {
            out.push_str("\nNo findings. The deck spec is clean and the PPTX is up to date.");
        }
        out
    }
}

pub fn deck_dir_for_spec_path(abs_path: &Path) -> Option<PathBuf> {
    let file_name = abs_path.file_name()?.to_str()?;
    if file_name.eq_ignore_ascii_case(MANIFEST_FILE) {
        return abs_path.parent().map(Path::to_path_buf);
    }
    let ext_ok = abs_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    if !ext_ok {
        return None;
    }
    let parent = abs_path.parent()?;
    if parent
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case(SLIDES_DIR))
        .unwrap_or(false)
    {
        let deck_dir = parent.parent()?;
        if deck_dir.join(MANIFEST_FILE).is_file() {
            return Some(deck_dir.to_path_buf());
        }
    }
    None
}

fn visual_len(text: &str) -> usize {
    text.chars()
        .map(|c| if (c as u32) < 0x2E80 { 1 } else { 2 })
        .sum()
}

fn workspace_rel(abs: &Path, workspace: &Path) -> Option<String> {
    let abs_c = std::fs::canonicalize(abs).unwrap_or_else(|_| abs.to_path_buf());
    let ws_c = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let strip = |p: &Path| -> PathBuf {
        let raw = p.to_string_lossy();
        if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = raw.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
        p.to_path_buf()
    };
    strip(&abs_c)
        .strip_prefix(strip(&ws_c))
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

struct ResolveCtx<'a> {
    theme: &'static DeckTheme,
    overrides: Option<&'a BTreeMap<String, String>>,
    stage_w: u32,
    slot_scale: f64,
    deck_dir: &'a Path,
    workspace: &'a Path,
}

impl ResolveCtx<'_> {
    fn color(&self, value: &str) -> Option<String> {
        theme::resolve_color(value, self.theme, self.overrides)
    }

    fn color_or(&self, value: Option<&str>, fallback_token: &str, out: &mut CompileOutcome, loc: &str) -> String {
        match value {
            Some(v) => match self.color(v) {
                Some(hex) => hex,
                None => {
                    out.push(
                        "P1",
                        loc.to_string(),
                        format!("Unknown color `{v}` — use a palette token (background/surface/text/muted/accent/accent2/hairline/onAccent) or a hex value."),
                    );
                    self.color(fallback_token).unwrap_or_else(|| "#000000".to_string())
                }
            },
            None => self.color(fallback_token).unwrap_or_else(|| "#000000".to_string()),
        }
    }

    fn slot_frame(&self, slot: &LayoutSlot) -> (f64, f64, f64, f64) {
        (
            slot.x * self.slot_scale,
            slot.y,
            slot.w * self.slot_scale,
            slot.h,
        )
    }
}

fn font_name(kind: FontKind) -> &'static str {
    match kind {
        FontKind::Heading => "heading",
        FontKind::Body => "body",
    }
}

fn font_kind(raw: Option<&str>, fallback: FontKind) -> FontKind {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("heading") => FontKind::Heading,
        Some("body") => FontKind::Body,
        _ => fallback,
    }
}

struct TextDefaults {
    size: f64,
    bold: bool,
    italic: bool,
    color: String,
    font: FontKind,
    line_spacing: f64,
    budget: usize,
    align: String,
}

fn text_defaults(
    block: &BlockSpec,
    slot: Option<&LayoutSlot>,
    ctx: &ResolveCtx<'_>,
    out: &mut CompileOutcome,
    loc: &str,
) -> TextDefaults {
    let role_name = block
        .role
        .as_deref()
        .map(str::to_string)
        .or_else(|| slot.map(|s| s.role.to_string()))
        .unwrap_or_else(|| "body".to_string());
    let preset = role_preset(&role_name);
    let color = ctx.color_or(block.color.as_deref(), preset.color_token, out, loc);
    TextDefaults {
        size: block.size.filter(|s| *s > 4.0).unwrap_or(preset.size),
        bold: block.bold.unwrap_or(preset.bold),
        italic: block.italic.unwrap_or(false),
        color,
        font: font_kind(block.font.as_deref(), preset.font),
        line_spacing: block
            .line_spacing
            .filter(|v| *v >= 0.8 && *v <= 3.0)
            .unwrap_or(preset.line_spacing),
        budget: preset.budget,
        align: block
            .align
            .clone()
            .or_else(|| slot.map(|s| s.align.to_string()))
            .unwrap_or_else(|| "left".to_string()),
    }
}

fn resolve_runs(
    block: &BlockSpec,
    defaults: &TextDefaults,
    ctx: &ResolveCtx<'_>,
    out: &mut CompileOutcome,
    loc: &str,
) -> Vec<RenderParagraph> {
    let mut paragraphs: Vec<RenderParagraph> = Vec::new();
    if let Some(runs) = &block.runs {
        let rendered: Vec<RenderRun> = runs
            .iter()
            .filter(|r| !r.text.trim().is_empty())
            .map(|r| RenderRun {
                text: r.text.clone(),
                bold: r.bold.unwrap_or(defaults.bold),
                italic: r.italic.unwrap_or(defaults.italic),
                color: r
                    .color
                    .as_deref()
                    .and_then(|c| ctx.color(c))
                    .unwrap_or_else(|| defaults.color.clone()),
                size: r.size.filter(|s| *s > 4.0).unwrap_or(defaults.size),
                font: font_name(font_kind(r.font.as_deref(), defaults.font)).to_string(),
            })
            .collect();
        if !rendered.is_empty() {
            paragraphs.push(RenderParagraph {
                bullet: false,
                level: 0,
                bullet_char: None,
                space_before: 0.0,
                runs: rendered,
            });
        }
    } else if let Some(text) = &block.text {
        for (i, line) in text.split('\n').enumerate() {
            let line = line.trim_end();
            if line.trim().is_empty() {
                continue;
            }
            paragraphs.push(RenderParagraph {
                bullet: false,
                level: 0,
                bullet_char: None,
                space_before: if i == 0 { 0.0 } else { defaults.size * 0.35 },
                runs: vec![RenderRun {
                    text: line.to_string(),
                    bold: defaults.bold,
                    italic: defaults.italic,
                    color: defaults.color.clone(),
                    size: defaults.size,
                    font: font_name(defaults.font).to_string(),
                }],
            });
        }
    }
    let total: usize = paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .map(|r| visual_len(&r.text))
        .sum();
    if total > defaults.budget * 3 {
        out.push(
            "P1",
            loc.to_string(),
            format!(
                "Text is far over budget ({total} visual chars, budget ~{}) — tighten the copy or split the slide.",
                defaults.budget
            ),
        );
    }
    paragraphs
}

fn resolve_bullets(
    block: &BlockSpec,
    defaults: &TextDefaults,
    ctx: &ResolveCtx<'_>,
    out: &mut CompileOutcome,
    loc: &str,
) -> Vec<RenderParagraph> {
    let marker = block
        .marker
        .clone()
        .unwrap_or_else(|| ctx.theme.bullet.to_string());
    let gap = block.gap.filter(|g| *g >= 0.0).unwrap_or(defaults.size * 0.6);
    let mut paragraphs = Vec::new();
    let Some(items) = &block.items else {
        return paragraphs;
    };
    if items.len() > 7 {
        out.push(
            "P1",
            loc.to_string(),
            format!("{} bullet items — keep it to at most 6 per slide (6x6 rule); split or regroup.", items.len()),
        );
    }
    for (i, item) in items.iter().enumerate() {
        let (text, level, bold, color, size) = match item {
            BulletItemSpec::Plain(s) => (s.clone(), 0u8, None, None, None),
            BulletItemSpec::Detailed {
                text,
                level,
                bold,
                color,
                size,
            } => (
                text.clone(),
                level.unwrap_or(0).min(2),
                *bold,
                color.clone(),
                *size,
            ),
        };
        if text.trim().is_empty() {
            continue;
        }
        if visual_len(&text) > 110 {
            out.push(
                "P1",
                loc.to_string(),
                format!("Bullet item {} is too long ({} visual chars) — keep each point under ~50 characters.", i + 1, visual_len(&text)),
            );
        }
        let item_size = size
            .filter(|s| *s > 4.0)
            .unwrap_or(if level > 0 { defaults.size * 0.88 } else { defaults.size });
        let color = color
            .as_deref()
            .and_then(|c| ctx.color(c))
            .unwrap_or_else(|| defaults.color.clone());
        paragraphs.push(RenderParagraph {
            bullet: true,
            level,
            bullet_char: Some(marker.clone()),
            space_before: if i == 0 { 0.0 } else { gap },
            runs: vec![RenderRun {
                text,
                bold: bold.unwrap_or(defaults.bold),
                italic: defaults.italic,
                color,
                size: item_size,
                font: font_name(defaults.font).to_string(),
            }],
        });
    }
    paragraphs
}

fn resolve_block(
    block: &BlockSpec,
    index: usize,
    layout: &str,
    ctx: &ResolveCtx<'_>,
    out: &mut CompileOutcome,
    slide_loc: &str,
) -> Option<RenderBlock> {
    let block_id = block
        .id
        .clone()
        .unwrap_or_else(|| format!("block-{}", index + 1));
    let loc = format!("{slide_loc} → block `{block_id}`");
    let kind = block.kind.as_deref().map(str::trim).unwrap_or("");
    let slot = block.slot.as_deref().and_then(|name| {
        theme::layout_slots(layout)
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
    });
    if block.slot.is_some() && slot.is_none() {
        let available: Vec<&str> = theme::layout_slots(layout).iter().map(|s| s.name).collect();
        out.push(
            "P1",
            loc.clone(),
            format!(
                "Unknown slot `{}` for layout `{layout}` (available: {}) — falling back to the explicit frame or a default area.",
                block.slot.as_deref().unwrap_or(""),
                available.join(", ")
            ),
        );
    }
    let frame = block
        .frame
        .map(|f| (f.x, f.y, f.w.max(1.0), f.h.max(1.0)))
        .or_else(|| slot.map(|s| ctx.slot_frame(s)));
    let Some((x, y, w, h)) = frame else {
        out.push(
            "P0",
            loc.clone(),
            "Block has neither a `frame` ({x,y,w,h} in stage pixels) nor a valid `slot` — it cannot be placed.".to_string(),
        );
        return None;
    };
    if block.frame.is_some() {
        let stage_w = ctx.stage_w as f64;
        if x < -4.0 || y < -4.0 || x + w > stage_w + 4.0 || y + h > 1084.0 {
            if !(x <= 0.0 && y <= 0.0 && x + w >= stage_w && y + h >= 1080.0) {
                out.push(
                    "P1",
                    loc.clone(),
                    format!(
                        "Frame ({x:.0},{y:.0} {w:.0}x{h:.0}) extends beyond the {stage_w:.0}x1080 stage — content will be cut off in the PPTX; fix the geometry (or make it an intentional full-bleed covering the whole stage)."
                    ),
                );
            }
        }
    }

    match kind.to_ascii_lowercase().as_str() {
        "text" => {
            let defaults = text_defaults(block, slot, ctx, out, &loc);
            let paragraphs = resolve_runs(block, &defaults, ctx, out, &loc);
            if paragraphs.is_empty() {
                out.push("P0", loc, "Text block has no `text` or `runs` content.".to_string());
                return None;
            }
            Some(RenderBlock::Text(RenderTextBlock {
                id: block_id,
                x,
                y,
                w,
                h,
                align: defaults.align.clone(),
                valign: block
                    .valign
                    .clone()
                    .unwrap_or_else(|| "top".to_string()),
                line_spacing: defaults.line_spacing,
                paragraphs,
            }))
        }
        "bullets" => {
            let defaults = text_defaults(block, slot, ctx, out, &loc);
            let paragraphs = resolve_bullets(block, &defaults, ctx, out, &loc);
            if paragraphs.is_empty() {
                out.push("P0", loc, "Bullets block has no `items`.".to_string());
                return None;
            }
            Some(RenderBlock::Text(RenderTextBlock {
                id: block_id,
                x,
                y,
                w,
                h,
                align: defaults.align.clone(),
                valign: block.valign.clone().unwrap_or_else(|| "top".to_string()),
                line_spacing: defaults.line_spacing,
                paragraphs,
            }))
        }
        "image" => {
            let Some(src) = block.src.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
                out.push("P0", loc, "Image block is missing `src`.".to_string());
                return None;
            };
            let normalized = src.replace('\\', "/");
            let mut candidates = vec![
                ctx.deck_dir.join(&normalized),
                ctx.workspace.join(&normalized),
            ];
            if let Some(parent) = ctx.deck_dir.parent() {
                candidates.push(parent.join(&normalized));
            }
            let resolved = candidates.iter().find(|p| p.is_file());
            let Some(abs) = resolved else {
                out.push(
                    "P0",
                    loc,
                    format!("Image file `{src}` not found (looked relative to the deck directory and the workspace root) — generate or fix the asset, or remove this block."),
                );
                return None;
            };
            let Some(rel) = workspace_rel(abs, ctx.workspace) else {
                out.push("P0", loc, format!("Image `{src}` resolves outside the workspace."));
                return None;
            };
            Some(RenderBlock::Image(RenderImageBlock {
                id: block_id,
                x,
                y,
                w,
                h,
                src: rel,
                fit: block
                    .fit
                    .as_deref()
                    .map(str::trim)
                    .filter(|f| f.eq_ignore_ascii_case("contain"))
                    .map(|_| "contain".to_string())
                    .unwrap_or_else(|| "cover".to_string()),
                radius: block.radius.filter(|r| *r >= 0.0).unwrap_or(0.0),
            }))
        }
        "shape" => {
            let shape = block
                .shape
                .as_deref()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| "rect".to_string());
            let shape = match shape.as_str() {
                "rect" | "rectangle" => "rect",
                "roundrect" | "round-rect" | "rounded" => "roundRect",
                "ellipse" | "circle" => "ellipse",
                "line" => "line",
                other => {
                    out.push(
                        "P1",
                        loc.clone(),
                        format!("Unknown shape `{other}` — using `rect` (supported: rect, roundRect, ellipse, line)."),
                    );
                    "rect"
                }
            };
            let fill = block.fill.as_ref().map(|f| RenderPaint {
                color: ctx.color_or(Some(f.color.as_str()), "surface", out, &loc),
                alpha: f.alpha.unwrap_or(1.0).clamp(0.0, 1.0),
            });
            let stroke = block.stroke.as_ref().map(|s| RenderStroke {
                color: ctx.color_or(Some(s.color.as_str()), "hairline", out, &loc),
                width: s.width.unwrap_or(1.0).max(0.25),
                alpha: s.alpha.unwrap_or(1.0).clamp(0.0, 1.0),
            });
            if fill.is_none() && stroke.is_none() {
                out.push(
                    "P0",
                    loc,
                    "Shape block needs `fill` and/or `stroke` — an invisible shape is not allowed.".to_string(),
                );
                return None;
            }
            Some(RenderBlock::Shape(RenderShapeBlock {
                id: block_id,
                x,
                y,
                w,
                h,
                shape: shape.to_string(),
                radius: block.radius.filter(|r| *r >= 0.0).unwrap_or(0.0),
                fill,
                stroke,
            }))
        }
        "table" => {
            let Some(rows) = block.rows.as_ref().filter(|r| !r.is_empty()) else {
                out.push("P0", loc, "Table block is missing `rows`.".to_string());
                return None;
            };
            let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
            if col_count == 0 {
                out.push("P0", loc, "Table rows are empty.".to_string());
                return None;
            }
            if rows.len() > 9 || col_count > 6 {
                out.push(
                    "P1",
                    loc.clone(),
                    format!("Table is dense ({} rows × {col_count} cols) — keep tables presentation-sized (≤8 rows, ≤5 cols).", rows.len()),
                );
            }
            let mut fracs: Vec<f64> = block
                .columns
                .clone()
                .unwrap_or_default()
                .into_iter()
                .filter(|v| *v > 0.0)
                .collect();
            if fracs.len() != col_count {
                fracs = vec![1.0; col_count];
            }
            let total: f64 = fracs.iter().sum();
            let fracs: Vec<f64> = fracs.iter().map(|v| v / total).collect();
            let normalized_rows: Vec<Vec<String>> = rows
                .iter()
                .map(|r| {
                    let mut cells = r.clone();
                    cells.resize(col_count, String::new());
                    cells
                })
                .collect();
            let defaults = text_defaults(block, slot, ctx, out, &loc);
            Some(RenderBlock::Table(RenderTableBlock {
                id: block_id,
                x,
                y,
                w,
                h,
                col_fracs: fracs,
                header_row: block.header_row.unwrap_or(true),
                rows: normalized_rows,
                size: block.size.filter(|s| *s > 4.0).unwrap_or(28.0),
                text_color: defaults.color,
                header_fill: ctx.color("accent").unwrap_or_else(|| "#333333".into()),
                header_text: ctx.color("onAccent").unwrap_or_else(|| "#FFFFFF".into()),
                row_fill: ctx.color("surface").unwrap_or_else(|| "#FFFFFF".into()),
                hairline: ctx.color("hairline").unwrap_or_else(|| "#DDDDDD".into()),
                font_css: ctx.theme.body_css.to_string(),
                font_latin: ctx.theme.body_latin.to_string(),
                font_ea: ctx.theme.body_ea.to_string(),
            }))
        }
        "" => {
            out.push("P0", loc, "Block is missing `type` (text | bullets | image | shape | table).".to_string());
            None
        }
        other => {
            out.push(
                "P0",
                loc,
                format!("Unknown block type `{other}` (supported: text, bullets, image, shape, table)."),
            );
            None
        }
    }
}

fn furniture_blocks(
    layout: &str,
    index: usize,
    total: usize,
    manifest: &DeckManifest,
    ctx: &ResolveCtx<'_>,
) -> Vec<RenderBlock> {
    let mut out = Vec::new();
    let skip = matches!(layout, "cover" | "ending" | "image-full");
    if skip {
        return out;
    }
    let muted = ctx.color("muted").unwrap_or_else(|| "#888888".into());
    let stage_w = ctx.stage_w as f64;
    if manifest.page_numbers.unwrap_or(true) {
        out.push(RenderBlock::Text(RenderTextBlock {
            id: "_page".to_string(),
            x: stage_w - 320.0,
            y: 1006.0,
            w: 200.0,
            h: 44.0,
            align: "right".to_string(),
            valign: "top".to_string(),
            line_spacing: 1.0,
            paragraphs: vec![RenderParagraph {
                bullet: false,
                level: 0,
                bullet_char: None,
                space_before: 0.0,
                runs: vec![RenderRun {
                    text: format!("{:02} / {:02}", index + 1, total),
                    bold: false,
                    italic: false,
                    color: muted.clone(),
                    size: 22.0,
                    font: "body".to_string(),
                }],
            }],
        }));
    }
    if let Some(footer) = manifest.footer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push(RenderBlock::Text(RenderTextBlock {
            id: "_footer".to_string(),
            x: 120.0,
            y: 1006.0,
            w: stage_w - 480.0,
            h: 44.0,
            align: "left".to_string(),
            valign: "top".to_string(),
            line_spacing: 1.0,
            paragraphs: vec![RenderParagraph {
                bullet: false,
                level: 0,
                bullet_char: None,
                space_before: 0.0,
                runs: vec![RenderRun {
                    text: footer.to_string(),
                    bold: false,
                    italic: false,
                    color: muted,
                    size: 22.0,
                    font: "body".to_string(),
                }],
            }],
        }));
    }
    out
}

fn resolve_background(
    slide: &SlideSpec,
    ctx: &ResolveCtx<'_>,
    out: &mut CompileOutcome,
    loc: &str,
) -> RenderBackground {
    if let Some(bg) = &slide.background {
        if let Some(img) = bg.image.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let normalized = img.replace('\\', "/");
            let candidates = [ctx.deck_dir.join(&normalized), ctx.workspace.join(&normalized)];
            if let Some(abs) = candidates.iter().find(|p| p.is_file()) {
                if let Some(rel) = workspace_rel(abs, ctx.workspace) {
                    return RenderBackground::Image { src: rel };
                }
            }
            out.push(
                "P0",
                loc.to_string(),
                format!("Background image `{img}` not found — generate the asset first or drop the `image` background."),
            );
        }
        if let Some(grad) = bg.gradient.as_ref().filter(|g| g.len() >= 2) {
            let from = ctx.color_or(Some(grad[0].as_str()), "background", out, loc);
            let to = ctx.color_or(Some(grad[1].as_str()), "accent", out, loc);
            return RenderBackground::Gradient {
                from,
                to,
                angle: bg.angle.unwrap_or(135.0),
            };
        }
        if let Some(color) = bg.color.as_deref() {
            return RenderBackground::Color {
                color: ctx.color_or(Some(color), "background", out, loc),
            };
        }
    }
    if let Some((from, to, angle)) = ctx.theme.background_gradient {
        return RenderBackground::Gradient {
            from: from.to_string(),
            to: to.to_string(),
            angle,
        };
    }
    RenderBackground::Color {
        color: ctx.color("background").unwrap_or_else(|| "#FFFFFF".into()),
    }
}

pub fn compile_deck(deck_dir: &Path, workspace: &Path) -> CompileOutcome {
    let mut out = CompileOutcome::default();
    let manifest_path = deck_dir.join(MANIFEST_FILE);
    let raw = match std::fs::read_to_string(&manifest_path) {
        Ok(r) => r,
        Err(e) => {
            out.push("P0", MANIFEST_FILE, format!("Cannot read deck manifest: {e}"));
            return out;
        }
    };
    let manifest: DeckManifest = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            out.push("P0", MANIFEST_FILE, format!("deck.json is not valid JSON for the deck schema: {e}"));
            return out;
        }
    };
    if manifest.slides.is_empty() {
        out.push("P0", MANIFEST_FILE, "`slides` is empty — list the slide ids in presentation order.".to_string());
        return out;
    }
    {
        let mut seen = std::collections::HashSet::new();
        for id in &manifest.slides {
            if !seen.insert(id.trim().to_ascii_lowercase()) {
                out.push("P1", MANIFEST_FILE, format!("Duplicate slide id `{id}` in `slides`."));
            }
        }
    }
    let theme_id = manifest.theme.as_deref().unwrap_or(theme::DEFAULT_THEME_ID);
    if !theme::is_known_theme(theme_id) {
        out.push(
            "P1",
            MANIFEST_FILE,
            format!(
                "Unknown theme `{theme_id}` — falling back to `{}`. Available: {}.",
                theme::DEFAULT_THEME_ID,
                theme::THEMES.iter().map(|t| t.id).collect::<Vec<_>>().join(", ")
            ),
        );
    }
    let deck_theme = theme::theme_for(theme_id);
    let (stage_w, stage_h) = stage_for_aspect(manifest.aspect.as_deref());
    if let Some(aspect) = manifest.aspect.as_deref() {
        if !matches!(aspect.trim(), "16:9" | "4:3") {
            out.push("P1", MANIFEST_FILE, format!("Unknown aspect `{aspect}` — using 16:9."));
        }
    }
    let ctx = ResolveCtx {
        theme: deck_theme,
        overrides: manifest.palette.as_ref(),
        stage_w,
        slot_scale: stage_w as f64 / 1920.0,
        deck_dir,
        workspace,
    };

    let title = manifest
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Slide deck")
        .to_string();
    if manifest.title.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_none() {
        out.push("P1", MANIFEST_FILE, "`title` is missing — set the deck title.".to_string());
    }

    let mut slides: Vec<RenderSlide> = Vec::new();
    let total = manifest.slides.len();
    for (index, slide_id) in manifest.slides.iter().enumerate() {
        let slide_id = slide_id.trim();
        let slide_path = deck_dir.join(SLIDES_DIR).join(format!("{slide_id}.json"));
        let loc = format!("slides/{slide_id}.json");
        let raw = match std::fs::read_to_string(&slide_path) {
            Ok(r) => r,
            Err(_) => {
                out.pending_slides.push(slide_id.to_string());
                continue;
            }
        };
        let spec: SlideSpec = match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(e) => {
                out.push("P0", loc, format!("Slide file is not valid JSON for the slide schema: {e}"));
                continue;
            }
        };
        let layout_raw = spec.layout.as_deref().unwrap_or("content");
        let layout = if theme::is_known_layout(layout_raw) {
            layout_raw.trim().to_ascii_lowercase()
        } else {
            out.push(
                "P1",
                loc.clone(),
                format!(
                    "Unknown layout `{layout_raw}` — using `content`. Available: {}.",
                    theme::LAYOUT_IDS.join(", ")
                ),
            );
            "content".to_string()
        };
        if spec.blocks.is_empty() {
            out.push("P0", loc.clone(), "Slide has no `blocks`.".to_string());
            continue;
        }
        if spec.blocks.len() > 14 {
            out.push(
                "P1",
                loc.clone(),
                format!("{} blocks on one slide — simplify (≤12 including decorations).", spec.blocks.len()),
            );
        }
        let background = resolve_background(&spec, &ctx, &mut out, &loc);
        {
            let mut seen_blocks = std::collections::HashSet::new();
            for block in &spec.blocks {
                if let Some(id) = block.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    if !seen_blocks.insert(id.to_ascii_lowercase()) {
                        out.push(
                            "P1",
                            loc.clone(),
                            format!("Duplicate block id `{id}` on this slide — block ids must be unique so canvas point-selects stay precise."),
                        );
                    }
                }
            }
        }
        let mut blocks: Vec<RenderBlock> = Vec::new();
        for (bi, block) in spec.blocks.iter().enumerate() {
            if let Some(rendered) = resolve_block(block, bi, &layout, &ctx, &mut out, &loc) {
                blocks.push(rendered);
            }
        }
        if blocks.is_empty() {
            out.push("P0", loc.clone(), "No renderable blocks survived validation on this slide.".to_string());
            continue;
        }
        blocks.extend(furniture_blocks(&layout, index, total, &manifest, &ctx));
        slides.push(RenderSlide {
            id: slide_id.to_string(),
            layout,
            background,
            notes: spec
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            blocks,
        });
    }

    out.slide_count = slides.len();
    if slides.len() < 3 && out.pending_slides.is_empty() {
        out.push("P1", MANIFEST_FILE, format!("Only {} slide(s) compiled — a complete deck needs at least cover, content and ending.", slides.len()));
    }
    if slides.is_empty() {
        return out;
    }

    let transition = manifest
        .transition
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|t| matches!(t.as_str(), "none" | "subtle" | "cinematic"))
        .unwrap_or_else(|| "subtle".to_string());

    let render = RenderDeck {
        version: 1,
        title,
        theme: deck_theme.id.to_string(),
        stage_w,
        stage_h,
        transition,
        accent: ctx
            .color("accent")
            .unwrap_or_else(|| deck_theme.colors.accent.to_string()),
        fonts: RenderFonts {
            heading_latin: manifest
                .fonts
                .as_ref()
                .and_then(|f| f.heading.clone())
                .unwrap_or_else(|| deck_theme.heading_latin.to_string()),
            heading_ea: deck_theme.heading_ea.to_string(),
            body_latin: manifest
                .fonts
                .as_ref()
                .and_then(|f| f.body.clone())
                .unwrap_or_else(|| deck_theme.body_latin.to_string()),
            body_ea: deck_theme.body_ea.to_string(),
            heading_css: deck_theme.heading_css.to_string(),
            body_css: deck_theme.body_css.to_string(),
        },
        slides,
    };

    let render_path = deck_dir.join(RENDER_FILE);
    match serde_json::to_string_pretty(&render) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&render_path, json) {
                out.push("P0", RENDER_FILE, format!("Failed to write render model: {e}"));
                return out;
            }
        }
        Err(e) => {
            out.push("P0", RENDER_FILE, format!("Failed to serialize render model: {e}"));
            return out;
        }
    }

    let pptx_path = deck_dir.join(PPTX_FILE);
    match super::pptx::write_render_pptx(&pptx_path, &render, workspace) {
        Ok(()) => {
            out.wrote_outputs = true;
            out.pptx_path = Some(pptx_path);
        }
        Err(e) => {
            out.push("P0", PPTX_FILE, format!("PPTX write failed: {e}"));
        }
    }
    out
}

pub fn compile_deck_quiet(deck_dir: &Path, workspace: &Path) {
    let _ = compile_deck(deck_dir, workspace);
}
