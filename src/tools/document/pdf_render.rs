// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use printpdf::{
    BuiltinFont, Color, Line, LinePoint, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions,
    Point, Pt, Rgb, TextItem,
};
use std::collections::BTreeSet;

use super::common::{is_table_row, is_table_separator, split_table_row};
use super::font_discovery;
use super::pdf_font::{self, EmbeddedPdfFont};

const PAGE_W_PT: f32 = 595.28;
const PAGE_H_PT: f32 = 841.89;
const MARGIN: f32 = 50.0;
const BODY_SIZE: f32 = 11.0;
const LINE_GAP: f32 = 4.0;
const FONT_COVERAGE_TARGET: f32 = 0.98;

pub struct PdfRenderResult {
    pub bytes: Vec<u8>,
    pub warning: Option<String>,
    pub embedded_font: Option<String>,
}

fn pt_point(x: f32, y: f32) -> Point {
    Point::new(Pt(x).into(), Pt(y).into())
}

enum Block {
    Heading { level: u8, text: String },
    Paragraph(String),
    Bullet(String),
    Table { rows: Vec<Vec<String>>, header: bool },
}

struct TextStyle<'a> {
    handle: PdfFontHandle,
    font: Option<&'a EmbeddedPdfFont>,
}

impl TextStyle<'_> {
    fn char_width(&self, ch: char, size: f32) -> f32 {
        if let Some(font) = self.font {
            if let Some(adv) = font.advance_em(ch) {
                return adv * size;
            }
        }
        fallback_char_width(ch, size)
    }

    fn text_width(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|c| self.char_width(c, size)).sum()
    }

    fn sanitize(&self, text: &str, replaced: &mut usize) -> String {
        let Some(font) = self.font else {
            return text.to_string();
        };
        text.chars()
            .map(|c| {
                if c == '\n' || c.is_control() || font.covers(c) {
                    c
                } else {
                    *replaced += 1;
                    '?'
                }
            })
            .collect()
    }
}

fn collect_chars(blocks: &[Block]) -> (BTreeSet<char>, BTreeSet<char>) {
    let mut regular: BTreeSet<char> = BTreeSet::new();
    let mut bold: BTreeSet<char> = BTreeSet::new();
    for block in blocks {
        match block {
            Block::Heading { text, .. } => bold.extend(text.chars()),
            Block::Paragraph(text) | Block::Bullet(text) => regular.extend(text.chars()),
            Block::Table { rows, header } => {
                for (i, row) in rows.iter().enumerate() {
                    let target = if *header && i == 0 {
                        &mut bold
                    } else {
                        &mut regular
                    };
                    for cell in row {
                        target.extend(cell.chars());
                    }
                }
            }
        }
    }
    regular.insert('\u{2022}');
    regular.insert('\u{2026}');
    bold.insert('\u{2026}');
    (regular, bold)
}

pub fn render_markdown_pdf(
    markdown: &str,
    title: Option<&str>,
    font_bytes: Option<Vec<u8>>,
) -> anyhow::Result<PdfRenderResult> {
    let blocks = parse_blocks(markdown);
    let (regular_chars, bold_chars) = collect_chars(&blocks);
    let all_chars: BTreeSet<char> = regular_chars.union(&bold_chars).copied().collect();
    let needs_embedding = font_bytes.is_some() || blocks_have_non_ascii(&blocks);

    let embedded: Option<(EmbeddedPdfFont, Option<EmbeddedPdfFont>)> = if !needs_embedding {
        None
    } else if let Some(bytes) = font_bytes {
        let (index, ratio) = pdf_font::best_face_index(&bytes, &all_chars, FONT_COVERAGE_TARGET)
            .ok_or_else(|| anyhow::anyhow!("could not parse the provided font file"))?;
        if ratio < 0.5 {
            return Err(anyhow::anyhow!(
                "the provided font only covers {:.0}% of the characters in the document; supply a font that supports the document's language (e.g. a CJK font for Chinese text)",
                ratio * 100.0
            ));
        }
        let font = pdf_font::load_embedded_font(&bytes, index, &all_chars)?;
        Some((font, None))
    } else {
        let discovered = font_discovery::discover_cjk_fonts(&all_chars).ok_or_else(|| {
            anyhow::anyhow!(
                "PDF 内容包含非 ASCII 文本，但系统中未找到可用的 CJK/Unicode 字体，无法生成可读的 PDF。请安装中文字体（如微软雅黑/思源黑体），或通过 `font_path` 参数指定一个 .ttf/.otf/.ttc 字体文件。(The content contains non-ASCII text but no usable system font was found; install a CJK font or pass `font_path`.)"
            )
        })?;
        let regular_font = pdf_font::load_embedded_font(
            &discovered.regular.bytes,
            discovered.regular.index,
            &all_chars,
        )?;
        let bold_font = discovered.bold.as_ref().and_then(|b| {
            let (index, _) = pdf_font::best_face_index(&b.bytes, &bold_chars, FONT_COVERAGE_TARGET)?;
            pdf_font::load_embedded_font(&b.bytes, index, &bold_chars).ok()
        });
        Some((regular_font, bold_font))
    };

    let mut doc = PdfDocument::new(title.unwrap_or("Document"));

    let (regular, bold, embedded_font_name) = match &embedded {
        Some((reg, bold_opt)) => {
            let reg_id = doc.add_font(&reg.parsed);
            let bold_style = match bold_opt {
                Some(b) => {
                    let bold_id = doc.add_font(&b.parsed);
                    TextStyle {
                        handle: PdfFontHandle::External(bold_id),
                        font: Some(b),
                    }
                }
                None => TextStyle {
                    handle: PdfFontHandle::External(reg_id.clone()),
                    font: Some(reg),
                },
            };
            (
                TextStyle {
                    handle: PdfFontHandle::External(reg_id),
                    font: Some(reg),
                },
                bold_style,
                Some(reg.font_name.clone()),
            )
        }
        None => (
            TextStyle {
                handle: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                font: None,
            },
            TextStyle {
                handle: PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
                font: None,
            },
            None,
        ),
    };

    let mut replaced_chars = 0usize;

    let content_w = PAGE_W_PT - 2.0 * MARGIN;
    let mut ops: Vec<Op> = Vec::new();
    let mut pages: Vec<PdfPage> = Vec::new();
    let mut y = PAGE_H_PT - MARGIN;

    let dark = Color::Rgb(Rgb {
        r: 0.1,
        g: 0.1,
        b: 0.1,
        icc_profile: None,
    });
    let hairline = Color::Rgb(Rgb {
        r: 0.75,
        g: 0.75,
        b: 0.78,
        icc_profile: None,
    });

    macro_rules! flush_page {
        () => {{
            if !ops.is_empty() {
                pages.push(PdfPage::new(
                    printpdf::Mm(210.0),
                    printpdf::Mm(297.0),
                    std::mem::take(&mut ops),
                ));
            }
        }};
    }

    macro_rules! ensure_space {
        ($needed:expr) => {{
            if y - $needed < MARGIN {
                flush_page!();
                y = PAGE_H_PT - MARGIN;
            }
        }};
    }

    for block in &blocks {
        match block {
            Block::Heading { level, text } => {
                let size = match level {
                    1 => 22.0,
                    2 => 17.0,
                    3 => 14.0,
                    _ => 12.0,
                };
                let lh = size + LINE_GAP;
                let text = bold.sanitize(text, &mut replaced_chars);
                for line in wrap_text(&bold, &text, size, content_w) {
                    ensure_space!(lh);
                    y -= size;
                    push_text(&mut ops, &line, MARGIN, y, size, &bold.handle, &dark);
                    y -= LINE_GAP;
                }
                y -= LINE_GAP;
            }
            Block::Paragraph(text) => {
                let lh = BODY_SIZE + LINE_GAP;
                let text = regular.sanitize(text, &mut replaced_chars);
                for line in wrap_text(&regular, &text, BODY_SIZE, content_w) {
                    ensure_space!(lh);
                    y -= BODY_SIZE;
                    push_text(&mut ops, &line, MARGIN, y, BODY_SIZE, &regular.handle, &dark);
                    y -= LINE_GAP;
                }
                y -= LINE_GAP;
            }
            Block::Bullet(text) => {
                let lh = BODY_SIZE + LINE_GAP;
                let indent = 16.0;
                let text = regular.sanitize(text, &mut replaced_chars);
                let lines = wrap_text(&regular, &text, BODY_SIZE, content_w - indent);
                for (i, line) in lines.iter().enumerate() {
                    ensure_space!(lh);
                    y -= BODY_SIZE;
                    if i == 0 {
                        push_text(
                            &mut ops,
                            "\u{2022}",
                            MARGIN,
                            y,
                            BODY_SIZE,
                            &regular.handle,
                            &dark,
                        );
                    }
                    push_text(
                        &mut ops,
                        line,
                        MARGIN + indent,
                        y,
                        BODY_SIZE,
                        &regular.handle,
                        &dark,
                    );
                    y -= LINE_GAP;
                }
            }
            Block::Table { rows, header } => {
                render_table(
                    &mut ops,
                    &mut pages,
                    &mut y,
                    rows,
                    *header,
                    content_w,
                    &regular,
                    &bold,
                    &dark,
                    &hairline,
                    &mut replaced_chars,
                );
                y -= LINE_GAP * 2.0;
            }
        }
    }

    flush_page!();
    if pages.is_empty() {
        pages.push(PdfPage::new(
            printpdf::Mm(210.0),
            printpdf::Mm(297.0),
            Vec::new(),
        ));
    }

    let warning = if replaced_chars > 0 {
        Some(format!(
            "{replaced_chars} character(s) were not covered by the embedded font and were replaced with '?'."
        ))
    } else {
        None
    };

    let bytes = doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    Ok(PdfRenderResult {
        bytes,
        warning,
        embedded_font: embedded_font_name,
    })
}

fn push_text(
    ops: &mut Vec<Op>,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    font: &PdfFontHandle,
    color: &Color,
) {
    if text.is_empty() {
        return;
    }
    ops.push(Op::StartTextSection);
    ops.push(Op::SetTextCursor {
        pos: pt_point(x, y),
    });
    ops.push(Op::SetFont {
        font: font.clone(),
        size: Pt(size),
    });
    ops.push(Op::SetFillColor { col: color.clone() });
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(text.to_string())],
    });
    ops.push(Op::EndTextSection);
}

fn line_op(ops: &mut Vec<Op>, x1: f32, y1: f32, x2: f32, y2: f32, color: &Color) {
    ops.push(Op::SetOutlineColor { col: color.clone() });
    ops.push(Op::SetOutlineThickness { pt: Pt(0.7) });
    ops.push(Op::DrawLine {
        line: Line {
            points: vec![
                LinePoint {
                    p: pt_point(x1, y1),
                    bezier: false,
                },
                LinePoint {
                    p: pt_point(x2, y2),
                    bezier: false,
                },
            ],
            is_closed: false,
        },
    });
}

#[allow(clippy::too_many_arguments)]
fn render_table(
    ops: &mut Vec<Op>,
    pages: &mut Vec<PdfPage>,
    y: &mut f32,
    rows: &[Vec<String>],
    header: bool,
    content_w: f32,
    regular: &TextStyle,
    bold: &TextStyle,
    dark: &Color,
    hairline: &Color,
    replaced: &mut usize,
) {
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return;
    }
    let col_w = content_w / ncols as f32;
    let pad = 4.0;
    let row_h = BODY_SIZE + 8.0;
    let cell_size = BODY_SIZE;

    for (r, row) in rows.iter().enumerate() {
        if *y - row_h < MARGIN {
            if !ops.is_empty() {
                pages.push(PdfPage::new(
                    printpdf::Mm(210.0),
                    printpdf::Mm(297.0),
                    std::mem::take(ops),
                ));
            }
            *y = PAGE_H_PT - MARGIN;
        }
        let top = *y;
        let bottom = *y - row_h;
        let is_header = header && r == 0;
        let style = if is_header { bold } else { regular };
        for c in 0..ncols {
            let cx = MARGIN + c as f32 * col_w;
            let raw = row.get(c).map(String::as_str).unwrap_or("");
            let clean = style.sanitize(raw, replaced);
            let text = truncate_to_width(style, &clean, cell_size, col_w - 2.0 * pad);
            push_text(
                ops,
                &text,
                cx + pad,
                bottom + pad + 1.0,
                cell_size,
                &style.handle,
                dark,
            );
        }
        line_op(ops, MARGIN, top, MARGIN + content_w, top, hairline);
        line_op(ops, MARGIN, bottom, MARGIN + content_w, bottom, hairline);
        for c in 0..=ncols {
            let cx = MARGIN + c as f32 * col_w;
            line_op(ops, cx, top, cx, bottom, hairline);
        }
        *y = bottom;
    }
}

fn is_wide(ch: char) -> bool {
    let c = ch as u32;
    (0x1100..=0x115F).contains(&c)
        || (0x2E80..=0xA4CF).contains(&c)
        || (0xAC00..=0xD7A3).contains(&c)
        || (0xF900..=0xFAFF).contains(&c)
        || (0xFF00..=0xFF60).contains(&c)
        || (0xFFE0..=0xFFE6).contains(&c)
}

fn fallback_char_width(ch: char, size: f32) -> f32 {
    if is_wide(ch) {
        size
    } else {
        size * 0.52
    }
}

fn wrap_text(style: &TextStyle, text: &str, size: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    if text.trim().is_empty() {
        return vec![String::new()];
    }
    let mut current = String::new();
    let mut current_w = 0.0f32;
    let flush = |cur: &mut String, lines: &mut Vec<String>| {
        lines.push(std::mem::take(cur));
    };
    for ch in text.chars() {
        if ch == '\n' {
            flush(&mut current, &mut lines);
            current_w = 0.0;
            continue;
        }
        let w = style.char_width(ch, size);
        if current_w + w > max_w && !current.is_empty() {
            if let Some(pos) = current.rfind(' ') {
                if !is_wide(ch) {
                    let rest = current.split_off(pos + 1);
                    let mut trimmed = std::mem::take(&mut current);
                    while trimmed.ends_with(' ') {
                        trimmed.pop();
                    }
                    lines.push(trimmed);
                    current = rest;
                    current_w = style.text_width(&current, size);
                } else {
                    flush(&mut current, &mut lines);
                    current_w = 0.0;
                }
            } else {
                flush(&mut current, &mut lines);
                current_w = 0.0;
            }
        }
        current.push(ch);
        current_w += w;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn truncate_to_width(style: &TextStyle, text: &str, size: f32, max_w: f32) -> String {
    if style.text_width(text, size) <= max_w {
        return text.to_string();
    }
    let ellipsis_w = style.char_width('\u{2026}', size);
    let mut out = String::new();
    let mut w = 0.0f32;
    for ch in text.chars() {
        let cw = style.char_width(ch, size);
        if w + cw + ellipsis_w > max_w {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('\u{2026}');
    out
}

fn blocks_have_non_ascii(blocks: &[Block]) -> bool {
    let check = |s: &str| s.chars().any(|c| !c.is_ascii());
    blocks.iter().any(|b| match b {
        Block::Heading { text, .. } => check(text),
        Block::Paragraph(t) | Block::Bullet(t) => check(t),
        Block::Table { rows, .. } => rows.iter().flatten().any(|c| check(c)),
    })
}

fn parse_blocks(markdown: &str) -> Vec<Block> {
    let lines: Vec<&str> = markdown.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut blocks = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") {
            i += 1;
            let mut code = Vec::new();
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push(lines[i].to_string());
                i += 1;
            }
            if i < lines.len() {
                i += 1;
            }
            for c in code {
                blocks.push(Block::Paragraph(c));
            }
            continue;
        }

        let mut heading = None;
        for level in (1..=6).rev() {
            let prefix = format!("{} ", "#".repeat(level));
            if let Some(rest) = trimmed.strip_prefix(&prefix) {
                heading = Some((level as u8, strip_inline(rest.trim())));
                break;
            }
        }
        if let Some((level, text)) = heading {
            blocks.push(Block::Heading { level, text });
            i += 1;
            continue;
        }

        if is_table_row(line) && i + 1 < lines.len() && is_table_separator(lines[i + 1]) {
            let header = split_table_row_clean(line);
            i += 2;
            let mut rows = vec![header];
            while i < lines.len() && is_table_row(lines[i]) {
                rows.push(split_table_row_clean(lines[i]));
                i += 1;
            }
            blocks.push(Block::Table { rows, header: true });
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            blocks.push(Block::Bullet(strip_inline(rest.trim())));
            i += 1;
            continue;
        }
        if let Some((num, rest)) = trimmed.split_once(". ") {
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                blocks.push(Block::Bullet(strip_inline(rest.trim())));
                i += 1;
                continue;
            }
        }

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        blocks.push(Block::Paragraph(strip_inline(trimmed)));
        i += 1;
    }
    blocks
}

fn strip_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '*' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            continue;
        }
        if ch == '`' || ch == '*' || ch == '_' {
            i += 1;
            continue;
        }
        if ch == '[' {
            if let Some((label, next)) = parse_link_label(&chars, i) {
                out.push_str(&label);
                i = next;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn parse_link_label(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut j = start + 1;
    let mut label = String::new();
    while j < chars.len() && chars[j] != ']' {
        label.push(chars[j]);
        j += 1;
    }
    if j >= chars.len() || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' {
        k += 1;
    }
    if k >= chars.len() || label.is_empty() {
        return None;
    }
    Some((label, k + 1))
}

fn split_table_row_clean(line: &str) -> Vec<String> {
    split_table_row(line)
        .iter()
        .map(|c| strip_inline(c))
        .collect()
}
