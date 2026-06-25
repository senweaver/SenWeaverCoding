// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use printpdf::{
    BuiltinFont, Color, Line, LinePoint, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Pt, Rgb, TextItem,
};

const PAGE_W_PT: f32 = 595.28;
const PAGE_H_PT: f32 = 841.89;
const MARGIN: f32 = 50.0;
const BODY_SIZE: f32 = 11.0;
const LINE_GAP: f32 = 4.0;

pub struct PdfRenderResult {
    pub bytes: Vec<u8>,
    pub warning: Option<String>,
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

pub fn render_markdown_pdf(
    markdown: &str,
    title: Option<&str>,
    font_bytes: Option<Vec<u8>>,
) -> anyhow::Result<PdfRenderResult> {
    let mut doc = PdfDocument::new(title.unwrap_or("Document"));

    let external = match font_bytes {
        Some(bytes) => match ParsedFont::from_bytes(&bytes, 0, &mut Vec::new()) {
            Some(parsed) => Some(doc.add_font(&parsed)),
            None => return Err(anyhow::anyhow!("could not parse the provided font file")),
        },
        None => None,
    };
    let regular = match &external {
        Some(id) => PdfFontHandle::External(id.clone()),
        None => PdfFontHandle::Builtin(BuiltinFont::Helvetica),
    };
    let bold = match &external {
        Some(id) => PdfFontHandle::External(id.clone()),
        None => PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
    };

    let blocks = parse_blocks(markdown);
    let needs_cjk = blocks_have_non_ascii(&blocks);
    let warning = if needs_cjk && external.is_none() {
        Some(
            "PDF contains non-ASCII text but no font was supplied; the built-in Helvetica cannot render CJK/Unicode glyphs. Pass `font_path` to a .ttf/.otf font for correct output."
                .to_string(),
        )
    } else {
        None
    };

    let content_w = PAGE_W_PT - 2.0 * MARGIN;
    let mut ops: Vec<Op> = Vec::new();
    let mut pages: Vec<PdfPage> = Vec::new();
    let mut y = PAGE_H_PT - MARGIN;

    let dark = Color::Rgb(Rgb { r: 0.1, g: 0.1, b: 0.1, icc_profile: None });
    let hairline = Color::Rgb(Rgb { r: 0.75, g: 0.75, b: 0.78, icc_profile: None });

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

    for block in blocks {
        match block {
            Block::Heading { level, text } => {
                let size = match level {
                    1 => 22.0,
                    2 => 17.0,
                    3 => 14.0,
                    _ => 12.0,
                };
                let lh = size + LINE_GAP;
                for line in wrap_text(&text, size, content_w) {
                    ensure_space!(lh);
                    y -= size;
                    push_text(&mut ops, &line, MARGIN, y, size, &bold, &dark);
                    y -= LINE_GAP;
                }
                y -= LINE_GAP;
            }
            Block::Paragraph(text) => {
                let lh = BODY_SIZE + LINE_GAP;
                for line in wrap_text(&text, BODY_SIZE, content_w) {
                    ensure_space!(lh);
                    y -= BODY_SIZE;
                    push_text(&mut ops, &line, MARGIN, y, BODY_SIZE, &regular, &dark);
                    y -= LINE_GAP;
                }
                y -= LINE_GAP;
            }
            Block::Bullet(text) => {
                let lh = BODY_SIZE + LINE_GAP;
                let indent = 16.0;
                let lines = wrap_text(&text, BODY_SIZE, content_w - indent);
                for (i, line) in lines.iter().enumerate() {
                    ensure_space!(lh);
                    y -= BODY_SIZE;
                    if i == 0 {
                        push_text(&mut ops, "\u{2022}", MARGIN, y, BODY_SIZE, &regular, &dark);
                    }
                    push_text(&mut ops, line, MARGIN + indent, y, BODY_SIZE, &regular, &dark);
                    y -= LINE_GAP;
                }
            }
            Block::Table { rows, header } => {
                render_table(
                    &mut ops, &mut pages, &mut y, &rows, header, content_w, &regular, &bold,
                    &dark, &hairline,
                );
                y -= LINE_GAP * 2.0;
            }
        }
    }

    flush_page!();
    if pages.is_empty() {
        pages.push(PdfPage::new(printpdf::Mm(210.0), printpdf::Mm(297.0), Vec::new()));
    }

    let bytes = doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    Ok(PdfRenderResult { bytes, warning })
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
    regular: &PdfFontHandle,
    bold: &PdfFontHandle,
    dark: &Color,
    hairline: &Color,
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
        let font = if is_header { bold } else { regular };
        for c in 0..ncols {
            let cx = MARGIN + c as f32 * col_w;
            let raw = row.get(c).map(String::as_str).unwrap_or("");
            let text = truncate_to_width(raw, cell_size, col_w - 2.0 * pad);
            push_text(ops, &text, cx + pad, bottom + pad + 1.0, cell_size, font, dark);
        }
        // Draw a full grid for THIS row band so borders stay correct across page breaks.
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

fn char_width(ch: char, size: f32) -> f32 {
    if is_wide(ch) {
        size
    } else {
        size * 0.52
    }
}

fn text_width(text: &str, size: f32) -> f32 {
    text.chars().map(|c| char_width(c, size)).sum()
}

fn wrap_text(text: &str, size: f32, max_w: f32) -> Vec<String> {
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
        let w = char_width(ch, size);
        if current_w + w > max_w && !current.is_empty() {
            // try to break at last space for latin
            if let Some(pos) = current.rfind(' ') {
                if !is_wide(ch) {
                    let rest = current.split_off(pos + 1);
                    let mut trimmed = std::mem::take(&mut current);
                    while trimmed.ends_with(' ') {
                        trimmed.pop();
                    }
                    lines.push(trimmed);
                    current = rest;
                    current_w = text_width(&current, size);
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

fn truncate_to_width(text: &str, size: f32, max_w: f32) -> String {
    if text_width(text, size) <= max_w {
        return text.to_string();
    }
    let ellipsis_w = char_width('\u{2026}', size);
    let mut out = String::new();
    let mut w = 0.0f32;
    for ch in text.chars() {
        let cw = char_width(ch, size);
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
            let header = split_table_row(line);
            i += 2;
            let mut rows = vec![header];
            while i < lines.len() && is_table_row(lines[i]) {
                rows.push(split_table_row(lines[i]));
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

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.matches('|').count() >= 2
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !is_table_row(t) {
        return false;
    }
    t.trim_matches('|')
        .split('|')
        .map(|c| c.trim())
        .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':' || c == ' '))
}

fn split_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|c| strip_inline(c.trim()))
        .collect()
}
