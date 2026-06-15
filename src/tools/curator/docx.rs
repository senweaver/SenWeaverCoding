// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::state::CuratorTemplateKind;
use std::path::Path;

#[cfg(feature = "tool-curator")]
pub fn render_docx(
    markdown: &str,
    template: CuratorTemplateKind,
    output_path: &Path,
) -> anyhow::Result<()> {
    render_docx_with_diagrams(markdown, template, output_path, &[])
}

#[cfg(feature = "tool-curator")]
pub fn render_docx_with_diagrams(
    markdown: &str,
    template: CuratorTemplateKind,
    output_path: &Path,
    diagrams: &[(String, std::path::PathBuf)],
) -> anyhow::Result<()> {
    use docx_rs::*;
    let diagram_map: std::collections::HashMap<String, std::path::PathBuf> = diagrams
        .iter()
        .map(|(code, path)| (normalize_diagram_code(code), path.clone()))
        .collect();
    let typo = typography_for(template);
    let meta = extract_metadata(markdown, template);

    let mut doc = Docx::new();

    doc = doc
        .default_fonts(
            RunFonts::new()
                .ascii(typo.body_font_ascii)
                .hi_ansi(typo.body_font_ascii)
                .east_asia(typo.body_font_east),
        )
        .default_size(typo.body_size_hp)
        .default_line_spacing(
            LineSpacing::new()
                .line(typo.line_spacing_twips)
                .line_rule(LineSpacingType::Auto),
        )
        .page_size(typo.page_width_twips, typo.page_height_twips)
        .page_margin(
            PageMargin::new()
                .top(typo.margin_top)
                .bottom(typo.margin_bottom)
                .left(typo.margin_left)
                .right(typo.margin_right)
                .header(typo.margin_header)
                .footer(typo.margin_footer),
        );

    doc = register_heading_styles(doc, &typo);
    doc = register_numbering_definitions(doc);

    doc = doc.header(build_header(&meta, &typo));
    doc = doc.footer(build_footer(&typo));

    if typo.first_page_different {
        doc = doc.title_pg();
        doc = doc.first_header(Header::new().add_paragraph(Paragraph::new()));
        doc = doc.first_footer(Footer::new().add_paragraph(Paragraph::new()));
    }

    if typo.include_cover_page {
        for p in cover_page_paragraphs(&meta, &typo) {
            doc = doc.add_paragraph(p);
        }
        doc = doc.add_paragraph(page_break_paragraph());
    }

    if typo.include_toc {
        doc = doc.add_paragraph(
            Paragraph::new()
                .style("Heading1")
                .add_run(heading_run(typo.toc_title, 1, &typo)),
        );
        doc = doc.add_table_of_contents(
            TableOfContents::new()
                .heading_styles_range(1, 3)
                .hyperlink()
                .auto(),
        );
        doc = doc.add_paragraph(page_break_paragraph());
    }

    let blocks = parse_blocks(markdown);
    let mut skip_first_h1 = typo.include_cover_page;
    let mut blank_run_after_code = false;
    let mut mermaid_idx = 0usize;
    let total_mermaid_blocks = blocks
        .iter()
        .filter(|block| {
            matches!(block, MdBlock::CodeBlock { lang, lines } if is_mermaid_block(lang, lines))
        })
        .count();
    let positional_fallback_ok = diagrams.len() == total_mermaid_blocks;

    for block in &blocks {
        match block {
            MdBlock::Heading { level, text } => {
                if *level == 1 && skip_first_h1 {
                    skip_first_h1 = false;
                    continue;
                }
                doc = doc.add_paragraph(
                    Paragraph::new()
                        .style(heading_style_name(*level))
                        .line_spacing(heading_spacing(*level, &typo))
                        .add_run(heading_run(text, *level, &typo)),
                );
                blank_run_after_code = false;
            }
            MdBlock::Paragraph(line) => {
                let mut para = emit_inline_runs(line, &typo);
                if typo.body_first_line_indent > 0 {
                    para = para.indent(
                        None,
                        Some(SpecialIndentType::FirstLine(typo.body_first_line_indent)),
                        None,
                        None,
                    );
                }
                doc = doc.add_paragraph(para);
                blank_run_after_code = false;
            }
            MdBlock::BulletList(items) => {
                for item in items {
                    doc = doc.add_paragraph(
                        emit_inline_runs(item, &typo)
                            .numbering(NumberingId::new(BULLET_NUM_ID), IndentLevel::new(0)),
                    );
                }
                blank_run_after_code = false;
            }
            MdBlock::OrderedList(items) => {
                for item in items {
                    doc = doc.add_paragraph(
                        emit_inline_runs(item, &typo)
                            .numbering(NumberingId::new(ORDERED_NUM_ID), IndentLevel::new(0)),
                    );
                }
                blank_run_after_code = false;
            }
            MdBlock::CodeBlock { lang, lines } => {
                let diagram_png = if !diagrams.is_empty() && is_mermaid_block(lang, lines) {
                    let by_code = diagram_map.get(&normalize_diagram_code(&lines.join("\n")));
                    let resolved = by_code.or_else(|| {
                        if positional_fallback_ok {
                            diagrams.get(mermaid_idx).map(|(_, p)| p)
                        } else {
                            None
                        }
                    });
                    mermaid_idx += 1;
                    resolved
                } else {
                    None
                };
                if let Some(png_path) = diagram_png {
                    if let Some(para) = embed_image_paragraph(
                        "",
                        &png_path.to_string_lossy(),
                        output_path.parent(),
                        &typo,
                    ) {
                        doc = doc.add_paragraph(para);
                        doc = doc.add_paragraph(
                            Paragraph::new()
                                .line_spacing(LineSpacing::new().before(40).after(40)),
                        );
                        blank_run_after_code = true;
                        continue;
                    }
                }
                doc = doc.add_table(render_code_block(lines, &typo));
                doc = doc.add_paragraph(
                    Paragraph::new().line_spacing(LineSpacing::new().before(40).after(40)),
                );
                blank_run_after_code = true;
            }
            MdBlock::Image { alt, path } => {
                let base = output_path.parent();
                if let Some(para) = embed_image_paragraph(alt, path, base, &typo) {
                    doc = doc.add_paragraph(para);
                    if !alt.trim().is_empty() {
                        doc = doc.add_paragraph(
                            Paragraph::new()
                                .align(AlignmentType::Center)
                                .add_run(
                                    Run::new()
                                        .add_text(alt.clone())
                                        .italic()
                                        .size(typo.body_size_hp - 2)
                                        .color("666666")
                                        .fonts(
                                            RunFonts::new()
                                                .ascii(typo.body_font_ascii)
                                                .hi_ansi(typo.body_font_ascii)
                                                .east_asia(typo.body_font_east),
                                        ),
                                ),
                        );
                    }
                } else {
                    doc = doc.add_paragraph(emit_inline_runs(
                        &format!("![{alt}]({path})"),
                        &typo,
                    ));
                }
                blank_run_after_code = false;
            }
            MdBlock::Table { header, rows } => {
                doc = doc.add_table(render_table(header, rows, &typo));
                doc = doc.add_paragraph(
                    Paragraph::new().line_spacing(
                        LineSpacing::new().before(60).after(60),
                    ),
                );
                blank_run_after_code = false;
            }
            MdBlock::Blank => {
                if !blank_run_after_code {
                    doc = doc.add_paragraph(Paragraph::new());
                }
                blank_run_after_code = false;
            }
            MdBlock::Blockquote(line) => {
                doc = doc.add_paragraph(
                    emit_inline_runs(line, &typo)
                        .indent(Some(480), None, Some(480), None)
                        .line_spacing(
                            LineSpacing::new()
                                .line(typo.line_spacing_twips)
                                .line_rule(LineSpacingType::Auto)
                                .before(60)
                                .after(60),
                        ),
                );
                blank_run_after_code = false;
            }
            MdBlock::HorizontalRule => {
                doc = doc.add_paragraph(
                    Paragraph::new()
                        .align(AlignmentType::Center)
                        .add_run(
                            Run::new()
                                .add_text("— — — — — — — — — — — — —")
                                .color("999999")
                                .size(typo.body_size_hp),
                        ),
                );
                blank_run_after_code = false;
            }
        }
    }

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(output_path)?;
    doc.build().pack(&mut file)?;
    Ok(())
}

#[cfg(not(feature = "tool-curator"))]
pub fn render_docx(
    _markdown: &str,
    _template: CuratorTemplateKind,
    _output_path: &Path,
) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "DOCX renderer disabled: rebuild with the `tool-curator` feature enabled"
    ))
}

#[cfg(not(feature = "tool-curator"))]
pub fn render_docx_with_diagrams(
    _markdown: &str,
    _template: CuratorTemplateKind,
    _output_path: &Path,
    _diagrams: &[(String, std::path::PathBuf)],
) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "DOCX renderer disabled: rebuild with the `tool-curator` feature enabled"
    ))
}

#[cfg(feature = "tool-curator")]
#[derive(Debug, Clone, Copy)]
struct Typography {
    body_font_ascii: &'static str,
    body_font_east: &'static str,
    heading_font_ascii: &'static str,
    heading_font_east: &'static str,
    code_font: &'static str,
    title_size_hp: usize,
    h1_size_hp: usize,
    h2_size_hp: usize,
    h3_size_hp: usize,
    body_size_hp: usize,
    code_size_hp: usize,
    line_spacing_twips: i32,
    page_width_twips: u32,
    page_height_twips: u32,
    margin_top: i32,
    margin_bottom: i32,
    margin_left: i32,
    margin_right: i32,
    margin_header: i32,
    margin_footer: i32,
    body_first_line_indent: i32,
    include_cover_page: bool,
    include_toc: bool,
    first_page_different: bool,
    running_head: &'static str,
    toc_title: &'static str,
}

#[cfg(feature = "tool-curator")]
const A4_WIDTH: u32 = 11906;
#[cfg(feature = "tool-curator")]
const A4_HEIGHT: u32 = 16838;
#[cfg(feature = "tool-curator")]
const LETTER_WIDTH: u32 = 12240;
#[cfg(feature = "tool-curator")]
const LETTER_HEIGHT: u32 = 15840;
#[cfg(feature = "tool-curator")]
const INCH: i32 = 1440;

#[cfg(feature = "tool-curator")]
const BULLET_NUM_ID: usize = 10;
#[cfg(feature = "tool-curator")]
const ORDERED_NUM_ID: usize = 11;

#[cfg(feature = "tool-curator")]
fn typography_for(template: CuratorTemplateKind) -> Typography {
    match template {
        CuratorTemplateKind::PaperApa => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "Times New Roman",
            heading_font_ascii: "Times New Roman",
            heading_font_east: "Times New Roman",
            code_font: "Consolas",
            title_size_hp: 24,
            h1_size_hp: 24,
            h2_size_hp: 24,
            h3_size_hp: 24,
            body_size_hp: 24,
            code_size_hp: 20,
            line_spacing_twips: 480,
            page_width_twips: LETTER_WIDTH,
            page_height_twips: LETTER_HEIGHT,
            margin_top: INCH,
            margin_bottom: INCH,
            margin_left: INCH,
            margin_right: INCH,
            margin_header: 720,
            margin_footer: 720,
            body_first_line_indent: 720,
            include_cover_page: true,
            include_toc: false,
            first_page_different: true,
            running_head: "RUNNING HEAD",
            toc_title: "Table of Contents",
        },
        CuratorTemplateKind::PaperMla => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "Times New Roman",
            heading_font_ascii: "Times New Roman",
            heading_font_east: "Times New Roman",
            code_font: "Consolas",
            title_size_hp: 24,
            h1_size_hp: 24,
            h2_size_hp: 24,
            h3_size_hp: 24,
            body_size_hp: 24,
            code_size_hp: 20,
            line_spacing_twips: 480,
            page_width_twips: LETTER_WIDTH,
            page_height_twips: LETTER_HEIGHT,
            margin_top: INCH,
            margin_bottom: INCH,
            margin_left: INCH,
            margin_right: INCH,
            margin_header: 720,
            margin_footer: 720,
            body_first_line_indent: 720,
            include_cover_page: false,
            include_toc: false,
            first_page_different: false,
            running_head: "",
            toc_title: "Table of Contents",
        },
        CuratorTemplateKind::PaperChicago => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "Times New Roman",
            heading_font_ascii: "Times New Roman",
            heading_font_east: "Times New Roman",
            code_font: "Consolas",
            title_size_hp: 24,
            h1_size_hp: 24,
            h2_size_hp: 24,
            h3_size_hp: 24,
            body_size_hp: 24,
            code_size_hp: 20,
            line_spacing_twips: 480,
            page_width_twips: LETTER_WIDTH,
            page_height_twips: LETTER_HEIGHT,
            margin_top: INCH,
            margin_bottom: INCH,
            margin_left: INCH,
            margin_right: INCH,
            margin_header: 720,
            margin_footer: 720,
            body_first_line_indent: 720,
            include_cover_page: true,
            include_toc: false,
            first_page_different: true,
            running_head: "",
            toc_title: "Table of Contents",
        },
        CuratorTemplateKind::PaperImrad => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "Times New Roman",
            heading_font_ascii: "Times New Roman",
            heading_font_east: "Times New Roman",
            code_font: "Consolas",
            title_size_hp: 28,
            h1_size_hp: 28,
            h2_size_hp: 26,
            h3_size_hp: 24,
            body_size_hp: 24,
            code_size_hp: 20,
            line_spacing_twips: 480,
            page_width_twips: LETTER_WIDTH,
            page_height_twips: LETTER_HEIGHT,
            margin_top: INCH,
            margin_bottom: INCH,
            margin_left: INCH,
            margin_right: INCH,
            margin_header: 720,
            margin_footer: 720,
            body_first_line_indent: 0,
            include_cover_page: false,
            include_toc: false,
            first_page_different: false,
            running_head: "",
            toc_title: "Table of Contents",
        },
        CuratorTemplateKind::PaperGb7714 => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "\u{5b8b}\u{4f53}",
            heading_font_ascii: "Times New Roman",
            heading_font_east: "\u{9ed1}\u{4f53}",
            code_font: "Consolas",
            title_size_hp: 44,
            h1_size_hp: 32,
            h2_size_hp: 28,
            h3_size_hp: 24,
            body_size_hp: 24,
            code_size_hp: 20,
            line_spacing_twips: 360,
            page_width_twips: A4_WIDTH,
            page_height_twips: A4_HEIGHT,
            margin_top: 1440,
            margin_bottom: 1440,
            margin_left: 1800,
            margin_right: 1800,
            margin_header: 851,
            margin_footer: 992,
            body_first_line_indent: 480,
            include_cover_page: true,
            include_toc: true,
            first_page_different: true,
            running_head: "",
            toc_title: "\u{76ee}\u{5f55}",
        },
        CuratorTemplateKind::SolutionFunctional
        | CuratorTemplateKind::SolutionGb8567_2006
        | CuratorTemplateKind::SolutionGb8567_1988 => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "\u{5b8b}\u{4f53}",
            heading_font_ascii: "Times New Roman",
            heading_font_east: "\u{9ed1}\u{4f53}",
            code_font: "Consolas",
            title_size_hp: 44,
            h1_size_hp: 32,
            h2_size_hp: 28,
            h3_size_hp: 24,
            body_size_hp: 24,
            code_size_hp: 20,
            line_spacing_twips: 360,
            page_width_twips: A4_WIDTH,
            page_height_twips: A4_HEIGHT,
            margin_top: 1440,
            margin_bottom: 1440,
            margin_left: 1800,
            margin_right: 1800,
            margin_header: 851,
            margin_footer: 992,
            body_first_line_indent: 480,
            include_cover_page: true,
            include_toc: true,
            first_page_different: true,
            running_head: "",
            toc_title: "\u{76ee}\u{5f55}",
        },
        CuratorTemplateKind::SolutionIeee830
        | CuratorTemplateKind::SolutionIso29148
        | CuratorTemplateKind::SolutionIso42010
        | CuratorTemplateKind::SolutionIeee1016
        | CuratorTemplateKind::SolutionIso12207 => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "Times New Roman",
            heading_font_ascii: "Calibri",
            heading_font_east: "Calibri",
            code_font: "Consolas",
            title_size_hp: 36,
            h1_size_hp: 28,
            h2_size_hp: 24,
            h3_size_hp: 22,
            body_size_hp: 22,
            code_size_hp: 20,
            line_spacing_twips: 276,
            page_width_twips: LETTER_WIDTH,
            page_height_twips: LETTER_HEIGHT,
            margin_top: INCH,
            margin_bottom: INCH,
            margin_left: INCH,
            margin_right: INCH,
            margin_header: 720,
            margin_footer: 720,
            body_first_line_indent: 0,
            include_cover_page: true,
            include_toc: true,
            first_page_different: true,
            running_head: "",
            toc_title: "Table of Contents",
        },
        CuratorTemplateKind::TechReport => Typography {
            body_font_ascii: "Times New Roman",
            body_font_east: "\u{5b8b}\u{4f53}",
            heading_font_ascii: "Calibri",
            heading_font_east: "\u{9ed1}\u{4f53}",
            code_font: "Consolas",
            title_size_hp: 36,
            h1_size_hp: 28,
            h2_size_hp: 24,
            h3_size_hp: 22,
            body_size_hp: 24,
            code_size_hp: 20,
            line_spacing_twips: 360,
            page_width_twips: A4_WIDTH,
            page_height_twips: A4_HEIGHT,
            margin_top: 1440,
            margin_bottom: 1440,
            margin_left: 1800,
            margin_right: 1800,
            margin_header: 851,
            margin_footer: 992,
            body_first_line_indent: 0,
            include_cover_page: true,
            include_toc: true,
            first_page_different: true,
            running_head: "",
            toc_title: "Table of Contents",
        },
    }
}

#[cfg(feature = "tool-curator")]
#[allow(dead_code)]
struct DocMetadata {
    title: String,
    standard_label: String,
    authors: String,
    affiliation: String,
    date: String,
    keywords: String,
    doc_id: String,
}

#[cfg(feature = "tool-curator")]
fn extract_metadata(markdown: &str, template: CuratorTemplateKind) -> DocMetadata {
    let title = extract_first_h1(markdown).unwrap_or_else(|| template_heading_label(template));
    let standard_label = template_heading_label(template);
    let mut authors = String::new();
    let mut affiliation = String::new();
    let mut date = String::new();
    let mut keywords = String::new();
    let mut doc_id = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(rest) = strip_bold_prefix(trimmed, "Authors") {
            authors = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "\u{4f5c}\u{8005}") {
            authors = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "Author") {
            authors = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "Affiliation") {
            affiliation = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "\u{4f5c}\u{8005}\u{5355}\u{4f4d}") {
            affiliation = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "Date") {
            date = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "\u{65e5}\u{671f}") {
            date = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "Keywords") {
            keywords = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "\u{5173}\u{952e}\u{8bcd}") {
            keywords = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "Report ID") {
            doc_id = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "\u{9879}\u{76ee}\u{7f16}\u{53f7}") {
            doc_id = rest;
        } else if let Some(rest) = strip_bold_prefix(trimmed, "Document ID") {
            doc_id = rest;
        }
    }
    if date.is_empty() || date.contains("YYYY") {
        date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    }
    DocMetadata {
        title,
        standard_label,
        authors,
        affiliation,
        date,
        keywords,
        doc_id,
    }
}

#[cfg(feature = "tool-curator")]
fn strip_bold_prefix(line: &str, prefix: &str) -> Option<String> {
    let patterns = [
        format!("**{prefix}**:"),
        format!("**{prefix}**："),
        format!("**{prefix}**"),
    ];
    for pat in &patterns {
        if let Some(rest) = line.strip_prefix(pat.as_str()) {
            return Some(rest.trim().trim_start_matches(':').trim_start_matches('\u{ff1a}').trim().to_string());
        }
    }
    None
}

#[cfg(feature = "tool-curator")]
fn template_heading_label(template: CuratorTemplateKind) -> String {
    let label = match template {
        CuratorTemplateKind::PaperImrad => "Academic Paper (IMRaD)",
        CuratorTemplateKind::PaperApa => "Academic Paper (APA 7th Edition)",
        CuratorTemplateKind::PaperMla => "Academic Paper (MLA 9th Edition)",
        CuratorTemplateKind::PaperChicago => "Academic Paper (Chicago 17/18th Edition)",
        CuratorTemplateKind::PaperGb7714 => "\u{5b66}\u{672f}\u{8bba}\u{6587} (GB/T 7714-2015 / 2025)",
        CuratorTemplateKind::SolutionFunctional => "\u{5de5}\u{7a0b}\u{89e3}\u{51b3}\u{65b9}\u{6848}\u{ff08}\u{529f}\u{80fd}\u{8bbe}\u{8ba1}\u{4e3a}\u{6838}\u{5fc3}\u{ff09}",
        CuratorTemplateKind::SolutionGb8567_2006 => "\u{8f6f}\u{4ef6}\u{89e3}\u{51b3}\u{65b9}\u{6848} (GB/T 8567-2006)",
        CuratorTemplateKind::SolutionGb8567_1988 => "\u{8f6f}\u{4ef6}\u{89e3}\u{51b3}\u{65b9}\u{6848} (GB/T 8567-1988)",
        CuratorTemplateKind::SolutionIeee830 => "Software Requirements Specification (IEEE 830-1998)",
        CuratorTemplateKind::SolutionIso29148 => "System/Software Requirements Specification (ISO/IEC/IEEE 29148:2011)",
        CuratorTemplateKind::SolutionIso42010 => "Software Architecture Description (ISO/IEC/IEEE 42010)",
        CuratorTemplateKind::SolutionIeee1016 => "Software Design Description (IEEE 1016-2009)",
        CuratorTemplateKind::SolutionIso12207 => "Software Lifecycle Process Plan (ISO/IEC/IEEE 12207)",
        CuratorTemplateKind::TechReport => "Technical Report",
    };
    label.to_string()
}

#[cfg(feature = "tool-curator")]
fn heading_style_name(level: u8) -> &'static str {
    match level {
        1 => "Heading1",
        2 => "Heading2",
        3 => "Heading3",
        4 => "Heading4",
        5 => "Heading5",
        _ => "Heading6",
    }
}

#[cfg(feature = "tool-curator")]
fn heading_size(level: u8, typo: &Typography) -> usize {
    match level {
        1 => typo.h1_size_hp,
        2 => typo.h2_size_hp,
        3 => typo.h3_size_hp,
        4 => typo.body_size_hp + 4,
        5 => typo.body_size_hp + 2,
        _ => typo.body_size_hp,
    }
}

#[cfg(feature = "tool-curator")]
fn register_heading_styles(mut doc: docx_rs::Docx, typo: &Typography) -> docx_rs::Docx {
    use docx_rs::*;
    for level in 1u8..=6 {
        let name = heading_style_name(level);
        let size_hp = heading_size(level, typo);
        let spacing = heading_spacing(level, typo);
        let mut style = Style::new(name, StyleType::Paragraph)
            .name(format!("heading {level}"))
            .size(size_hp)
            .fonts(
                RunFonts::new()
                    .ascii(typo.heading_font_ascii)
                    .hi_ansi(typo.heading_font_ascii)
                    .east_asia(typo.heading_font_east),
            )
            .line_spacing(spacing)
            .outline_lvl(level as usize - 1);
        if level <= 4 {
            style = style.bold();
        }
        if level >= 5 {
            style = style.italic();
        }
        if level >= 4 {
            style = style.color("404040");
        }
        doc = doc.add_style(style);
    }
    doc
}

#[cfg(feature = "tool-curator")]
fn register_numbering_definitions(mut doc: docx_rs::Docx) -> docx_rs::Docx {
    use docx_rs::*;
    let bullet_abs = AbstractNumbering::new(BULLET_NUM_ID)
        .add_level(
            Level::new(
                0,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new("\u{2022}"),
                LevelJc::new("left"),
            )
            .indent(Some(720), Some(SpecialIndentType::Hanging(360)), None, None),
        );
    let ordered_abs = AbstractNumbering::new(ORDERED_NUM_ID)
        .add_level(
            Level::new(
                0,
                Start::new(1),
                NumberFormat::new("decimal"),
                LevelText::new("%1."),
                LevelJc::new("left"),
            )
            .indent(Some(720), Some(SpecialIndentType::Hanging(360)), None, None),
        );
    doc = doc
        .add_abstract_numbering(bullet_abs)
        .add_numbering(docx_rs::Numbering::new(BULLET_NUM_ID, BULLET_NUM_ID))
        .add_abstract_numbering(ordered_abs)
        .add_numbering(docx_rs::Numbering::new(ORDERED_NUM_ID, ORDERED_NUM_ID));
    doc
}

#[cfg(feature = "tool-curator")]
fn build_header(meta: &DocMetadata, typo: &Typography) -> docx_rs::Header {
    use docx_rs::*;
    let mut para = Paragraph::new().align(AlignmentType::Right);
    if !typo.running_head.is_empty() {
        para = para.add_run(
            Run::new()
                .add_text(typo.running_head)
                .size(18)
                .fonts(
                    RunFonts::new()
                        .ascii(typo.body_font_ascii)
                        .hi_ansi(typo.body_font_ascii),
                ),
        );
        para = para.add_run(Run::new().add_text("   "));
    } else if !meta.title.is_empty() {
        let short_title: String = meta.title.chars().take(50).collect();
        para = para.add_run(
            Run::new()
                .add_text(short_title)
                .size(18)
                .color("666666")
                .fonts(
                    RunFonts::new()
                        .ascii(typo.body_font_ascii)
                        .hi_ansi(typo.body_font_ascii)
                        .east_asia(typo.body_font_east),
                ),
        );
        para = para.add_run(Run::new().add_text("   "));
    }
    para = para.add_page_num(PageNum::new());
    Header::new().add_paragraph(para)
}

#[cfg(feature = "tool-curator")]
fn build_footer(typo: &Typography) -> docx_rs::Footer {
    use docx_rs::*;
    let para = Paragraph::new()
        .align(AlignmentType::Center)
        .add_run(
            Run::new()
                .add_text("— ")
                .size(18)
                .color("999999")
                .fonts(
                    RunFonts::new()
                        .ascii(typo.body_font_ascii)
                        .hi_ansi(typo.body_font_ascii),
                ),
        )
        .add_page_num(PageNum::new())
        .add_run(
            Run::new()
                .add_text(" / ")
                .size(18)
                .color("999999"),
        )
        .add_num_pages(NumPages::new())
        .add_run(
            Run::new()
                .add_text(" —")
                .size(18)
                .color("999999"),
        );
    Footer::new().add_paragraph(para)
}

#[cfg(feature = "tool-curator")]
fn cover_page_paragraphs(
    meta: &DocMetadata,
    typo: &Typography,
) -> Vec<docx_rs::Paragraph> {
    use docx_rs::*;
    let mut out: Vec<Paragraph> = Vec::new();
    for _ in 0..5 {
        out.push(Paragraph::new().line_spacing(LineSpacing::new().line(480).line_rule(LineSpacingType::Auto)));
    }
    out.push(
        Paragraph::new()
            .align(AlignmentType::Center)
            .add_run(
                Run::new()
                    .add_text(&meta.title)
                    .bold()
                    .size(typo.title_size_hp)
                    .fonts(
                        RunFonts::new()
                            .ascii(typo.heading_font_ascii)
                            .hi_ansi(typo.heading_font_ascii)
                            .east_asia(typo.heading_font_east),
                    ),
            ),
    );
    out.push(Paragraph::new());
    if !meta.authors.is_empty() && !meta.authors.contains('<') {
        out.push(
            Paragraph::new()
                .align(AlignmentType::Center)
                .add_run(
                    Run::new()
                        .add_text(&meta.authors)
                        .size(typo.body_size_hp)
                        .fonts(
                            RunFonts::new()
                                .ascii(typo.body_font_ascii)
                                .hi_ansi(typo.body_font_ascii)
                                .east_asia(typo.body_font_east),
                        ),
                ),
        );
    }
    if !meta.affiliation.is_empty() && !meta.affiliation.contains('<') {
        out.push(
            Paragraph::new()
                .align(AlignmentType::Center)
                .add_run(
                    Run::new()
                        .add_text(&meta.affiliation)
                        .size(typo.body_size_hp)
                        .fonts(
                            RunFonts::new()
                                .ascii(typo.body_font_ascii)
                                .hi_ansi(typo.body_font_ascii)
                                .east_asia(typo.body_font_east),
                        ),
                ),
        );
    }
    if !meta.date.is_empty() {
        out.push(
            Paragraph::new()
                .align(AlignmentType::Center)
                .add_run(
                    Run::new()
                        .add_text(&meta.date)
                        .size(typo.body_size_hp)
                        .fonts(
                            RunFonts::new()
                                .ascii(typo.body_font_ascii)
                                .hi_ansi(typo.body_font_ascii)
                                .east_asia(typo.body_font_east),
                        ),
                ),
        );
    }
    out.push(Paragraph::new());
    out.push(Paragraph::new());
    out.push(
        Paragraph::new()
            .align(AlignmentType::Center)
            .add_run(
                Run::new()
                    .add_text(&meta.standard_label)
                    .size(typo.body_size_hp - 2)
                    .color("666666")
                    .fonts(
                        RunFonts::new()
                            .ascii(typo.body_font_ascii)
                            .hi_ansi(typo.body_font_ascii)
                            .east_asia(typo.body_font_east),
                    ),
            ),
    );
    if !meta.doc_id.is_empty() && !meta.doc_id.contains('<') {
        out.push(
            Paragraph::new()
                .align(AlignmentType::Center)
                .add_run(
                    Run::new()
                        .add_text(format!("Document ID: {}", meta.doc_id))
                        .size(typo.body_size_hp - 2)
                        .color("666666")
                        .fonts(
                            RunFonts::new()
                                .ascii(typo.body_font_ascii)
                                .hi_ansi(typo.body_font_ascii)
                                .east_asia(typo.body_font_east),
                        ),
                ),
        );
    }
    out
}

#[cfg(feature = "tool-curator")]
fn page_break_paragraph() -> docx_rs::Paragraph {
    use docx_rs::*;
    Paragraph::new().add_run(Run::new().add_break(BreakType::Page))
}

#[cfg(feature = "tool-curator")]
fn heading_spacing(level: u8, _typo: &Typography) -> docx_rs::LineSpacing {
    use docx_rs::*;
    match level {
        1 => LineSpacing::new().before(360).after(200).line(276).line_rule(LineSpacingType::Auto),
        2 => LineSpacing::new().before(240).after(120).line(276).line_rule(LineSpacingType::Auto),
        3 => LineSpacing::new().before(200).after(80).line(276).line_rule(LineSpacingType::Auto),
        _ => LineSpacing::new().before(160).after(60).line(276).line_rule(LineSpacingType::Auto),
    }
}

#[cfg(feature = "tool-curator")]
fn heading_run(text: &str, level: u8, typo: &Typography) -> docx_rs::Run {
    use docx_rs::*;
    let size = heading_size(level, typo);
    let mut run = Run::new()
        .add_text(text)
        .size(size)
        .fonts(
            RunFonts::new()
                .ascii(typo.heading_font_ascii)
                .hi_ansi(typo.heading_font_ascii)
                .east_asia(typo.heading_font_east),
        );
    if level <= 4 {
        run = run.bold();
    }
    if level >= 5 {
        run = run.italic();
    }
    if level >= 4 {
        run = run.color("404040");
    }
    run
}

#[cfg(feature = "tool-curator")]
fn extract_first_h1(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let clean = rest.trim().to_string();
            if !clean.is_empty() {
                return Some(clean);
            }
        }
    }
    None
}

#[cfg(feature = "tool-curator")]
fn render_table(
    header: &[String],
    rows: &[Vec<String>],
    typo: &Typography,
) -> docx_rs::Table {
    use docx_rs::*;
    let col_count = header.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if col_count == 0 {
        return Table::new(vec![]);
    }
    let mut docx_rows: Vec<TableRow> = Vec::new();
    let mut header_cells: Vec<TableCell> = Vec::new();
    for c in 0..col_count {
        let text = header.get(c).cloned().unwrap_or_default();
        header_cells.push(make_table_cell(&text, typo, true, false));
    }
    docx_rows.push(TableRow::new(header_cells));
    for (r, row) in rows.iter().enumerate() {
        let zebra = r % 2 == 1;
        let mut cells: Vec<TableCell> = Vec::new();
        for c in 0..col_count {
            let text = row.get(c).cloned().unwrap_or_default();
            cells.push(make_table_cell(&text, typo, false, zebra));
        }
        docx_rows.push(TableRow::new(cells));
    }
    Table::new(docx_rows)
        .layout(TableLayoutType::Autofit)
        .set_borders(
            TableBorders::new()
                .set(
                    TableBorder::new(TableBorderPosition::Top)
                        .border_type(BorderType::Single)
                        .size(8),
                )
                .set(
                    TableBorder::new(TableBorderPosition::Bottom)
                        .border_type(BorderType::Single)
                        .size(8),
                )
                .set(
                    TableBorder::new(TableBorderPosition::Left)
                        .border_type(BorderType::Single)
                        .size(4),
                )
                .set(
                    TableBorder::new(TableBorderPosition::Right)
                        .border_type(BorderType::Single)
                        .size(4),
                )
                .set(
                    TableBorder::new(TableBorderPosition::InsideH)
                        .border_type(BorderType::Single)
                        .size(4),
                )
                .set(
                    TableBorder::new(TableBorderPosition::InsideV)
                        .border_type(BorderType::Single)
                        .size(4),
                ),
        )
}

#[cfg(feature = "tool-curator")]
fn make_table_cell(
    text: &str,
    typo: &Typography,
    is_header: bool,
    zebra: bool,
) -> docx_rs::TableCell {
    use docx_rs::*;
    let para = table_cell_paragraph(text, typo, is_header);
    let mut cell = TableCell::new().add_paragraph(para);
    if is_header {
        cell = cell.shading(Shading::new().fill("E8E8E8"));
    } else if zebra {
        cell = cell.shading(Shading::new().fill("F6F6F8"));
    }
    cell
}

#[cfg(feature = "tool-curator")]
fn table_cell_paragraph(
    text: &str,
    typo: &Typography,
    is_header: bool,
) -> docx_rs::Paragraph {
    use docx_rs::*;
    let mut run = Run::new()
        .add_text(text)
        .fonts(
            RunFonts::new()
                .ascii(typo.body_font_ascii)
                .hi_ansi(typo.body_font_ascii)
                .east_asia(typo.body_font_east),
        )
        .size(typo.body_size_hp);
    if is_header {
        run = run.bold();
    }
    Paragraph::new()
        .line_spacing(
            LineSpacing::new()
                .line(240)
                .line_rule(LineSpacingType::Auto)
                .before(30)
                .after(30),
        )
        .add_run(run)
}

#[cfg(feature = "tool-curator")]
fn render_code_block(lines: &[String], typo: &Typography) -> docx_rs::Table {
    use docx_rs::*;
    let mut cell = TableCell::new().shading(Shading::new().fill("F4F4F5"));
    if lines.is_empty() {
        cell = cell.add_paragraph(Paragraph::new());
    }
    for line in lines {
        let run = Run::new()
            .add_text(line.clone())
            .fonts(
                RunFonts::new()
                    .ascii(typo.code_font)
                    .hi_ansi(typo.code_font)
                    .east_asia(typo.code_font),
            )
            .size(typo.code_size_hp)
            .color("1A1A1A");
        let para = Paragraph::new()
            .line_spacing(
                LineSpacing::new()
                    .line(typo.code_size_hp as i32 * 12)
                    .line_rule(LineSpacingType::Auto)
                    .before(10)
                    .after(10),
            )
            .add_run(run);
        cell = cell.add_paragraph(para);
    }
    let row = TableRow::new(vec![cell]);
    Table::new(vec![row])
        .layout(TableLayoutType::Autofit)
        .set_borders(
            TableBorders::new()
                .set(
                    TableBorder::new(TableBorderPosition::Top)
                        .border_type(BorderType::Single)
                        .color("D0D0D5")
                        .size(4),
                )
                .set(
                    TableBorder::new(TableBorderPosition::Bottom)
                        .border_type(BorderType::Single)
                        .color("D0D0D5")
                        .size(4),
                )
                .set(
                    TableBorder::new(TableBorderPosition::Left)
                        .border_type(BorderType::Single)
                        .color("D0D0D5")
                        .size(4),
                )
                .set(
                    TableBorder::new(TableBorderPosition::Right)
                        .border_type(BorderType::Single)
                        .color("D0D0D5")
                        .size(4),
                ),
        )
}

#[cfg(feature = "tool-curator")]
fn content_width_emu(typo: &Typography) -> u32 {
    let content_twips = (typo.page_width_twips as i32) - typo.margin_left - typo.margin_right;
    let content_twips = content_twips.max(2000) as u32;
    content_twips.saturating_mul(635)
}

#[cfg(feature = "tool-curator")]
fn embed_image_paragraph(
    alt: &str,
    rel_path: &str,
    base: Option<&Path>,
    typo: &Typography,
) -> Option<docx_rs::Paragraph> {
    use docx_rs::*;
    let _ = alt;
    if rel_path.starts_with("http://") || rel_path.starts_with("https://") || rel_path.starts_with("data:") {
        return None;
    }
    let candidate = std::path::Path::new(rel_path);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base?.join(candidate)
    };
    let bytes = std::fs::read(&resolved).ok()?;
    let img = ::image::load_from_memory(&bytes).ok()?;
    let (w_px, h_px) = ::image::GenericImageView::dimensions(&img);
    if w_px == 0 || h_px == 0 {
        return None;
    }
    let mut png_buf = std::io::Cursor::new(Vec::<u8>::new());
    img.write_to(&mut png_buf, ::image::ImageFormat::Png).ok()?;
    let png_bytes = png_buf.into_inner();

    const EMU_PER_PX: u64 = 9525;
    let mut w_emu = (w_px as u64) * EMU_PER_PX;
    let mut h_emu = (h_px as u64) * EMU_PER_PX;
    let max_w = content_width_emu(typo) as u64;
    if w_emu > max_w && w_emu > 0 {
        h_emu = h_emu.saturating_mul(max_w) / w_emu;
        w_emu = max_w;
    }
    let pic = Pic::new_with_dimensions(png_bytes, w_px, h_px)
        .size(w_emu.min(u32::MAX as u64) as u32, h_emu.min(u32::MAX as u64) as u32);
    Some(
        Paragraph::new()
            .align(AlignmentType::Center)
            .line_spacing(LineSpacing::new().before(80).after(40))
            .add_run(Run::new().add_image(pic)),
    )
}

#[cfg(feature = "tool-curator")]
fn normalize_diagram_code(code: &str) -> String {
    let lines: Vec<&str> = code
        .lines()
        .map(|l| l.trim_end_matches(['\r', ' ', '\t']))
        .collect();
    let mut start = 0usize;
    let mut end = lines.len();
    while start < end && lines[start].is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].is_empty() {
        end -= 1;
    }
    lines[start..end].join("\n").trim().to_string()
}

#[cfg(feature = "tool-curator")]
fn first_diagram_line(lines: &[String]) -> Option<&str> {
    let mut in_directive = false;
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if in_directive {
            if line.contains("}%%") {
                in_directive = false;
            }
            continue;
        }
        if line.starts_with("%%{") {
            if !line.contains("}%%") {
                in_directive = true;
            }
            continue;
        }
        if line.starts_with("%%") {
            continue;
        }
        return Some(line);
    }
    None
}

#[cfg(feature = "tool-curator")]
fn is_mermaid_block(lang: &str, lines: &[String]) -> bool {
    const DIAGRAM_TOKENS: &[&str] = &[
        "graph",
        "flowchart",
        "flowchart-elk",
        "sequencediagram",
        "classdiagram",
        "classdiagram-v2",
        "statediagram",
        "statediagram-v2",
        "erdiagram",
        "journey",
        "gantt",
        "pie",
        "gitgraph",
        "mindmap",
        "timeline",
        "requirement",
        "requirementdiagram",
        "quadrantchart",
        "xychart",
        "xychart-beta",
        "sankey",
        "sankey-beta",
        "block-beta",
        "packet",
        "packet-beta",
        "radar",
        "radar-beta",
        "treemap",
        "treemap-beta",
        "treeview-beta",
        "venn-beta",
        "wardley-beta",
        "eventmodeling",
        "ishikawa",
        "ishikawa-beta",
        "architecture",
        "architecture-beta",
        "kanban",
        "c4context",
        "c4container",
        "c4component",
        "c4dynamic",
        "c4deployment",
    ];
    let lang_key = lang.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if lang_key == "mermaid" {
        return true;
    }
    if lang_key.is_empty() || lang_key == "text" || lang_key == "plaintext" || lang_key == "plain" {
        if let Some(first) = first_diagram_line(lines) {
            let token = first
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches([':', ';'])
                .to_ascii_lowercase();
            return DIAGRAM_TOKENS.iter().any(|s| token == *s);
        }
    }
    false
}

#[cfg(feature = "tool-curator")]
fn parse_image_line(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let rest = t.strip_prefix("![")?;
    let close_alt = rest.find("](")?;
    let alt = rest[..close_alt].to_string();
    let after = &rest[close_alt + 2..];
    let close_paren = after.rfind(')')?;
    if close_paren + 1 != after.len() {
        return None;
    }
    let path_part = after[..close_paren].trim();
    let path = path_part
        .split_whitespace()
        .next()
        .unwrap_or(path_part)
        .trim_matches('"')
        .trim_matches('<')
        .trim_matches('>')
        .to_string();
    if path.is_empty() {
        return None;
    }
    Some((alt, path))
}

#[cfg(feature = "tool-curator")]
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum MdBlock {
    Heading { level: u8, text: String },
    Paragraph(String),
    BulletList(Vec<String>),
    OrderedList(Vec<String>),
    CodeBlock { lang: String, lines: Vec<String> },
    Table { header: Vec<String>, rows: Vec<Vec<String>> },
    Image { alt: String, path: String },
    Blockquote(String),
    HorizontalRule,
    Blank,
}

#[cfg(feature = "tool-curator")]
fn parse_blocks(markdown: &str) -> Vec<MdBlock> {
    let lines: Vec<&str> = markdown.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut blocks: Vec<MdBlock> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            let lang = rest.trim().to_string();
            let mut body: Vec<String> = Vec::new();
            i += 1;
            while i < lines.len() {
                let cur = lines[i].trim_end_matches('\r');
                if cur.trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                body.push(cur.to_string());
                i += 1;
            }
            blocks.push(MdBlock::CodeBlock { lang, lines: body });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("###### ") {
            blocks.push(MdBlock::Heading { level: 6, text: rest.trim().to_string() });
            i += 1; continue;
        }
        if let Some(rest) = trimmed.strip_prefix("##### ") {
            blocks.push(MdBlock::Heading { level: 5, text: rest.trim().to_string() });
            i += 1; continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#### ") {
            blocks.push(MdBlock::Heading { level: 4, text: rest.trim().to_string() });
            i += 1; continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            blocks.push(MdBlock::Heading { level: 3, text: rest.trim().to_string() });
            i += 1; continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            blocks.push(MdBlock::Heading { level: 2, text: rest.trim().to_string() });
            i += 1; continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            blocks.push(MdBlock::Heading { level: 1, text: rest.trim().to_string() });
            i += 1; continue;
        }
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            blocks.push(MdBlock::HorizontalRule);
            i += 1; continue;
        }
        if let Some((alt, path)) = parse_image_line(trimmed) {
            blocks.push(MdBlock::Image { alt, path });
            i += 1; continue;
        }
        if let Some(rest) = trimmed.strip_prefix("> ") {
            blocks.push(MdBlock::Blockquote(rest.to_string()));
            i += 1; continue;
        }
        if is_table_row(line) && i + 1 < lines.len() && is_table_separator(lines[i + 1]) {
            let header = split_table_row(line);
            let mut rows: Vec<Vec<String>> = Vec::new();
            i += 2;
            while i < lines.len() && is_table_row(lines[i]) {
                rows.push(split_table_row(lines[i]));
                i += 1;
            }
            blocks.push(MdBlock::Table { header, rows });
            continue;
        }
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let mut items: Vec<String> = Vec::new();
            while i < lines.len() {
                let cur = lines[i].trim_start();
                if let Some(rest) = cur.strip_prefix("- ") {
                    items.push(rest.to_string());
                    i += 1;
                } else if let Some(rest) = cur.strip_prefix("* ") {
                    items.push(rest.to_string());
                    i += 1;
                } else {
                    break;
                }
            }
            blocks.push(MdBlock::BulletList(items));
            continue;
        }
        if let Some((num_part, rest_after)) = trimmed.split_once(". ") {
            if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
                let mut items: Vec<String> = Vec::new();
                items.push(rest_after.to_string());
                i += 1;
                while i < lines.len() {
                    let cur = lines[i].trim_start();
                    if let Some((np, rest2)) = cur.split_once(". ") {
                        if !np.is_empty() && np.chars().all(|c| c.is_ascii_digit()) {
                            items.push(rest2.to_string());
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                blocks.push(MdBlock::OrderedList(items));
                continue;
            }
        }
        if trimmed.is_empty() {
            blocks.push(MdBlock::Blank);
            i += 1; continue;
        }
        blocks.push(MdBlock::Paragraph(line.to_string()));
        i += 1;
    }
    blocks
}

#[cfg(feature = "tool-curator")]
fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.matches('|').count() >= 2
}

#[cfg(feature = "tool-curator")]
fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !is_table_row(t) { return false; }
    let inner = t.trim_matches('|');
    inner.split('|').map(|c| c.trim()).all(|cell| {
        !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':' || c == ' ')
    })
}

#[cfg(feature = "tool-curator")]
fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    trimmed.split('|').map(|c| c.trim().to_string()).collect()
}

#[cfg(feature = "tool-curator")]
fn styled_run(
    text: &str,
    bold: bool,
    italic: bool,
    code: bool,
    typo: &Typography,
) -> docx_rs::Run {
    use docx_rs::*;
    let mut run = Run::new().add_text(text);
    if code {
        run = run
            .fonts(
                RunFonts::new()
                    .ascii(typo.code_font)
                    .hi_ansi(typo.code_font)
                    .east_asia(typo.code_font),
            )
            .size(typo.code_size_hp);
    } else {
        run = run
            .fonts(
                RunFonts::new()
                    .ascii(typo.body_font_ascii)
                    .hi_ansi(typo.body_font_ascii)
                    .east_asia(typo.body_font_east),
            )
            .size(typo.body_size_hp);
    }
    if bold {
        run = run.bold();
    }
    if italic {
        run = run.italic();
    }
    run
}

#[cfg(feature = "tool-curator")]
fn try_parse_inline_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if chars.get(start) != Some(&'[') {
        return None;
    }
    let mut j = start + 1;
    let mut text = String::new();
    while j < chars.len() && chars[j] != ']' {
        text.push(chars[j]);
        j += 1;
    }
    if j >= chars.len() || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let mut k = j + 2;
    let mut url = String::new();
    while k < chars.len() && chars[k] != ')' {
        url.push(chars[k]);
        k += 1;
    }
    if k >= chars.len() {
        return None;
    }
    let url = url.trim().to_string();
    if text.is_empty() || url.is_empty() {
        return None;
    }
    Some((text, url, k + 1))
}

#[cfg(feature = "tool-curator")]
fn emit_inline_runs(line: &str, typo: &Typography) -> docx_rs::Paragraph {
    use docx_rs::*;
    let mut para = Paragraph::new().line_spacing(
        LineSpacing::new()
            .line(typo.line_spacing_twips)
            .line_rule(LineSpacingType::Auto),
    );
    let chars: Vec<char> = line.chars().collect();
    let mut buf = String::new();
    let mut bold = false;
    let mut italic = false;
    let mut code = false;
    let mut i = 0usize;
    macro_rules! flush_buf {
        () => {{
            if !buf.is_empty() {
                para = para.add_run(styled_run(&buf, bold, italic, code, typo));
                buf.clear();
            }
        }};
    }
    while i < chars.len() {
        let ch = chars[i];
        if ch == '`' {
            flush_buf!();
            code = !code;
            i += 1;
            continue;
        }
        if !code && ch == '[' {
            if let Some((text, url, next)) = try_parse_inline_link(&chars, i) {
                flush_buf!();
                let link_run = Run::new()
                    .add_text(text)
                    .color("0563C1")
                    .underline("single")
                    .size(typo.body_size_hp)
                    .fonts(
                        RunFonts::new()
                            .ascii(typo.body_font_ascii)
                            .hi_ansi(typo.body_font_ascii)
                            .east_asia(typo.body_font_east),
                    );
                para = para.add_hyperlink(
                    Hyperlink::new(url, HyperlinkType::External).add_run(link_run),
                );
                i = next;
                continue;
            }
        }
        if !code && ch == '*' && chars.get(i + 1) == Some(&'*') {
            flush_buf!();
            bold = !bold;
            i += 2;
            continue;
        }
        if !code && ch == '_' && chars.get(i + 1) == Some(&'_') {
            flush_buf!();
            bold = !bold;
            i += 2;
            continue;
        }
        if !code && (ch == '*' || ch == '_') {
            flush_buf!();
            italic = !italic;
            i += 1;
            continue;
        }
        buf.push(ch);
        i += 1;
    }
    flush_buf!();
    para
}
