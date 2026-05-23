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
    use docx_rs::*;
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

    for block in &blocks {
        match block {
            MdBlock::Heading { level, text } => {
                if *level == 1 && skip_first_h1 {
                    skip_first_h1 = false;
                    continue;
                }
                let style_name = match level {
                    1 => "Heading1",
                    2 => "Heading2",
                    3 => "Heading3",
                    _ => "Heading3",
                };
                doc = doc.add_paragraph(
                    Paragraph::new()
                        .style(style_name)
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
            MdBlock::CodeBlock { lines, .. } => {
                for (idx, line) in lines.iter().enumerate() {
                    let mut run = Run::new()
                        .add_text(line.clone())
                        .fonts(
                            RunFonts::new()
                                .ascii(typo.code_font)
                                .hi_ansi(typo.code_font)
                                .east_asia(typo.code_font),
                        )
                        .size(typo.code_size_hp);
                    if idx == 0 {
                        run = run.color("1A1A1A");
                    }
                    let para = Paragraph::new()
                        .line_spacing(
                            LineSpacing::new()
                                .line(240)
                                .line_rule(LineSpacingType::Auto),
                        )
                        .indent(Some(420), None, None, None)
                        .add_run(run);
                    doc = doc.add_paragraph(para);
                }
                blank_run_after_code = true;
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

// ============================================================================
// Typography profile per standard
// ============================================================================

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

// ============================================================================
// Metadata extraction from markdown frontmatter
// ============================================================================

#[cfg(feature = "tool-curator")]
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

// ============================================================================
// Heading styles registration
// ============================================================================

#[cfg(feature = "tool-curator")]
fn register_heading_styles(mut doc: docx_rs::Docx, typo: &Typography) -> docx_rs::Docx {
    use docx_rs::*;
    for (level, name, size_hp) in [
        (1u8, "Heading1", typo.h1_size_hp),
        (2, "Heading2", typo.h2_size_hp),
        (3, "Heading3", typo.h3_size_hp),
    ] {
        let spacing = heading_spacing(level, typo);
        let style = Style::new(name, StyleType::Paragraph)
            .name(&format!("heading {level}"))
            .bold()
            .size(size_hp)
            .fonts(
                RunFonts::new()
                    .ascii(typo.heading_font_ascii)
                    .hi_ansi(typo.heading_font_ascii)
                    .east_asia(typo.heading_font_east),
            )
            .line_spacing(spacing)
            .outline_lvl(level as usize);
        doc = doc.add_style(style);
    }
    doc
}

// ============================================================================
// Numbering definitions (bullet + ordered)
// ============================================================================

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

// ============================================================================
// Header / Footer with page numbers
// ============================================================================

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

// ============================================================================
// Cover page
// ============================================================================

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

// ============================================================================
// Paragraph / Run helpers
// ============================================================================

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
    let size = match level {
        1 => typo.h1_size_hp,
        2 => typo.h2_size_hp,
        3 => typo.h3_size_hp,
        _ => typo.body_size_hp + 2,
    };
    Run::new()
        .add_text(text)
        .bold()
        .size(size)
        .fonts(
            RunFonts::new()
                .ascii(typo.heading_font_ascii)
                .hi_ansi(typo.heading_font_ascii)
                .east_asia(typo.heading_font_east),
        )
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

// ============================================================================
// Table rendering
// ============================================================================

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
        header_cells.push(make_table_cell(&text, typo, true));
    }
    docx_rows.push(TableRow::new(header_cells));
    for row in rows {
        let mut cells: Vec<TableCell> = Vec::new();
        for c in 0..col_count {
            let text = row.get(c).cloned().unwrap_or_default();
            cells.push(make_table_cell(&text, typo, false));
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
fn make_table_cell(text: &str, typo: &Typography, is_header: bool) -> docx_rs::TableCell {
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
    let para = Paragraph::new()
        .line_spacing(
            LineSpacing::new()
                .line(240)
                .line_rule(LineSpacingType::Auto)
                .before(30)
                .after(30),
        )
        .add_run(run);
    let mut cell = TableCell::new().add_paragraph(para);
    if is_header {
        cell = cell.shading(Shading::new().fill("E8E8E8"));
    }
    cell
}

// ============================================================================
// Markdown parsing
// ============================================================================

#[cfg(feature = "tool-curator")]
#[derive(Debug, Clone)]
enum MdBlock {
    Heading { level: u8, text: String },
    Paragraph(String),
    BulletList(Vec<String>),
    OrderedList(Vec<String>),
    CodeBlock { lang: String, lines: Vec<String> },
    Table { header: Vec<String>, rows: Vec<Vec<String>> },
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

// ============================================================================
// Inline formatting (bold, italic, code, links)
// ============================================================================

#[cfg(feature = "tool-curator")]
fn emit_inline_runs(line: &str, typo: &Typography) -> docx_rs::Paragraph {
    use docx_rs::*;
    let mut para = Paragraph::new().line_spacing(
        LineSpacing::new()
            .line(typo.line_spacing_twips)
            .line_rule(LineSpacingType::Auto),
    );
    let mut buf = String::new();
    let mut chars = line.chars().peekable();
    let mut bold = false;
    let mut italic = false;
    let mut code = false;
    let flush = |para: Paragraph,
                 buf: &mut String,
                 bold: bool,
                 italic: bool,
                 code: bool,
                 typo: &Typography|
     -> Paragraph {
        if buf.is_empty() { return para; }
        let mut run = Run::new().add_text(buf.clone());
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
        if bold { run = run.bold(); }
        if italic { run = run.italic(); }
        buf.clear();
        para.add_run(run)
    };
    while let Some(ch) = chars.next() {
        if ch == '`' {
            para = flush(para, &mut buf, bold, italic, code, typo);
            code = !code;
            continue;
        }
        if ch == '*' && chars.peek() == Some(&'*') {
            chars.next();
            para = flush(para, &mut buf, bold, italic, code, typo);
            bold = !bold;
            continue;
        }
        if ch == '_' && chars.peek() == Some(&'_') {
            chars.next();
            para = flush(para, &mut buf, bold, italic, code, typo);
            bold = !bold;
            continue;
        }
        if (ch == '*' || ch == '_') && !code {
            para = flush(para, &mut buf, bold, italic, code, typo);
            italic = !italic;
            continue;
        }
        buf.push(ch);
    }
    flush(para, &mut buf, bold, italic, code, typo)
}
