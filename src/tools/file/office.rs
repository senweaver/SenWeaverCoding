// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    Docx,
    Xlsx,
    Pptx,
    Pdf,
}

pub fn detect_office_kind_by_ext(path: &str) -> Option<OfficeKind> {
    let ext = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .rsplit('.')
        .next()
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("docx") => Some(OfficeKind::Docx),
        Some("xlsx") => Some(OfficeKind::Xlsx),
        Some("pptx") => Some(OfficeKind::Pptx),
        Some("pdf") => Some(OfficeKind::Pdf),
        _ => None,
    }
}

pub fn extract_office_text(kind: OfficeKind, bytes: &[u8]) -> anyhow::Result<Option<String>> {
    match kind {
        OfficeKind::Pdf => extract_pdf(bytes),
        OfficeKind::Docx => extract_docx(bytes),
        OfficeKind::Xlsx => extract_xlsx(bytes),
        OfficeKind::Pptx => extract_pptx(bytes),
    }
}

pub fn extract_pdf_text_if_pdf(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 5 || &bytes[..5] != b"%PDF-" {
        return None;
    }
    match extract_pdf(bytes) {
        Ok(Some(text)) if !text.trim().is_empty() => Some(text),
        _ => None,
    }
}

#[cfg(feature = "rag-pdf")]
fn extract_pdf(bytes: &[u8]) -> anyhow::Result<Option<String>> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| anyhow::anyhow!("PDF text extraction failed: {e}"))?;
    Ok(Some(reconstruct_pdf_tables(&text)))
}

#[cfg(feature = "rag-pdf")]
fn pdf_split_columns(line: &str) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut space_run = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            if !current.trim().is_empty() {
                cells.push(current.trim().to_string());
            }
            current.clear();
            space_run = 0;
            continue;
        }
        if ch == ' ' {
            space_run += 1;
            current.push(ch);
            continue;
        }
        if space_run >= 3 {
            if !current.trim().is_empty() {
                cells.push(current.trim().to_string());
            }
            current.clear();
        }
        space_run = 0;
        current.push(ch);
    }
    if !current.trim().is_empty() {
        cells.push(current.trim().to_string());
    }
    cells
}

#[cfg(feature = "rag-pdf")]
fn pdf_cell_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(feature = "rag-pdf")]
fn reconstruct_pdf_tables(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len());
    let mut noted = false;
    let mut i = 0usize;
    while i < lines.len() {
        let cells = pdf_split_columns(lines[i]);
        if cells.len() >= 2 {
            let ncols = cells.len();
            let mut group: Vec<Vec<String>> = vec![cells];
            let mut j = i + 1;
            while j < lines.len() {
                let next = pdf_split_columns(lines[j]);
                if next.len() == ncols {
                    group.push(next);
                    j += 1;
                } else {
                    break;
                }
            }
            // Only treat as a table when at least two consecutive lines share the SAME
            // column count; this avoids mangling ordinary prose that happens to contain
            // wide gaps. Otherwise fall through and emit the original line verbatim.
            if group.len() >= 2 {
                if !noted {
                    out.push_str(
                        "> Note: the table(s) below were heuristically reconstructed from the PDF text layout and may need review.\n\n",
                    );
                    noted = true;
                }
                let ncols = group.iter().map(|r| r.len()).max().unwrap_or(0);
                let fmt_row = |row: &[String]| -> String {
                    let cells: Vec<String> = (0..ncols)
                        .map(|c| pdf_cell_escape(row.get(c).map(String::as_str).unwrap_or("")))
                        .collect();
                    format!("| {} |", cells.join(" | "))
                };
                out.push_str(&fmt_row(&group[0]));
                out.push('\n');
                out.push_str(&format!("| {} |\n", vec!["---"; ncols].join(" | ")));
                for row in group.iter().skip(1) {
                    out.push_str(&fmt_row(row));
                    out.push('\n');
                }
                out.push('\n');
                i = j;
                continue;
            }
        }
        out.push_str(lines[i]);
        out.push('\n');
        i += 1;
    }
    out
}

#[cfg(not(feature = "rag-pdf"))]
fn extract_pdf(_bytes: &[u8]) -> anyhow::Result<Option<String>> {
    Ok(None)
}

#[cfg(feature = "office-docs")]
fn extract_docx(bytes: &[u8]) -> anyhow::Result<Option<String>> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("not a valid .docx (zip) file: {e}"))?;
    let mut raw = Vec::new();
    {
        let mut file = archive
            .by_name("word/document.xml")
            .map_err(|e| anyhow::anyhow!("missing word/document.xml: {e}"))?;
        file.read_to_end(&mut raw)
            .map_err(|e| anyhow::anyhow!("failed to read word/document.xml: {e}"))?;
    }
    let xml = String::from_utf8_lossy(&raw);
    let text = docx_xml_to_markdown(&xml)?;
    Ok(Some(text))
}

#[cfg(feature = "office-docs")]
fn heading_level_from_style(val: &str) -> Option<u8> {
    let key = val.trim().to_ascii_lowercase().replace([' ', '-', '_'], "");
    if key == "title" {
        return Some(1);
    }
    let digits = if let Some(rest) = key.strip_prefix("heading") {
        rest
    } else if key.chars().all(|c| c.is_ascii_digit()) {
        key.as_str()
    } else {
        return None;
    };
    match digits.parse::<u8>() {
        Ok(n) if (1..=6).contains(&n) => Some(n),
        _ => None,
    }
}

#[cfg(feature = "office-docs")]
fn markdown_table_cell(text: &str) -> String {
    text.replace(['\r', '\n', '\t'], " ")
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(feature = "office-docs")]
fn emit_markdown_table(out: &mut String, rows: &[Vec<String>]) {
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return;
    }
    let cell = |row: &[String], c: usize| -> String {
        markdown_table_cell(row.get(c).map(String::as_str).unwrap_or(""))
    };
    let header = &rows[0];
    out.push_str("| ");
    out.push_str(
        &(0..col_count)
            .map(|c| cell(header, c))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    out.push_str(" |\n");
    out.push_str("| ");
    out.push_str(&vec!["---"; col_count].join(" | "));
    out.push_str(" |\n");
    for row in rows.iter().skip(1) {
        out.push_str("| ");
        out.push_str(
            &(0..col_count)
                .map(|c| cell(row, c))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        out.push_str(" |\n");
    }
    out.push('\n');
}

#[cfg(feature = "office-docs")]
fn docx_xml_to_markdown(xml: &str) -> anyhow::Result<String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut out = String::new();

    let mut table_depth: u32 = 0;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut cell_text = String::new();
    let mut in_cell = false;

    let mut para_text = String::new();
    let mut heading_level: Option<u8> = None;
    let mut in_ppr = false;
    let mut capture_text = false;

    let read_style_val = |e: &quick_xml::events::BytesStart| -> Option<u8> {
        for attr in e.attributes().flatten() {
            let key = attr.key.as_ref();
            if key == b"w:val" || key.ends_with(b":val") || key == b"val" {
                let val = String::from_utf8_lossy(attr.value.as_ref());
                return heading_level_from_style(&val);
            }
        }
        None
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"w:tbl" => {
                    table_depth += 1;
                    if table_depth == 1 {
                        table_rows.clear();
                    }
                }
                b"w:tr" if table_depth > 0 => {
                    row_cells.clear();
                }
                b"w:tc" if table_depth > 0 => {
                    cell_text.clear();
                    in_cell = true;
                }
                b"w:pPr" => in_ppr = true,
                b"w:pStyle" => {
                    if in_ppr {
                        if let Some(level) = read_style_val(&e) {
                            heading_level = Some(level);
                        }
                    }
                }
                b"w:t" => capture_text = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"w:pStyle" => {
                    if in_ppr {
                        if let Some(level) = read_style_val(&e) {
                            heading_level = Some(level);
                        }
                    }
                }
                b"w:tab" => {
                    if in_cell {
                        cell_text.push(' ');
                    } else {
                        para_text.push('\t');
                    }
                }
                b"w:br" | b"w:cr" => {
                    if in_cell {
                        cell_text.push(' ');
                    } else {
                        para_text.push('\n');
                    }
                }
                _ => {}
            },
            Ok(Event::Text(e)) => {
                if capture_text {
                    if let Ok(txt) = e.xml_content(quick_xml::XmlVersion::Implicit1_0) {
                        if in_cell {
                            cell_text.push_str(&txt);
                        } else {
                            para_text.push_str(&txt);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"w:t" => capture_text = false,
                b"w:pPr" => in_ppr = false,
                b"w:p" => {
                    if in_cell {
                        cell_text.push(' ');
                        heading_level = None;
                    } else {
                        let trimmed = para_text.trim();
                        if !trimmed.is_empty() {
                            match heading_level {
                                Some(level) => {
                                    out.push_str(&"#".repeat(level as usize));
                                    out.push(' ');
                                    out.push_str(trimmed);
                                    out.push_str("\n\n");
                                }
                                None => {
                                    out.push_str(trimmed);
                                    out.push('\n');
                                }
                            }
                        }
                        para_text.clear();
                        heading_level = None;
                    }
                }
                b"w:tc" if table_depth > 0 => {
                    in_cell = false;
                    row_cells.push(cell_text.trim().to_string());
                    cell_text.clear();
                }
                b"w:tr" if table_depth > 0 => {
                    if !row_cells.is_empty() {
                        table_rows.push(std::mem::take(&mut row_cells));
                    }
                }
                b"w:tbl" => {
                    if table_depth > 0 {
                        table_depth -= 1;
                    }
                    if table_depth == 0 && !table_rows.is_empty() {
                        out.push('\n');
                        emit_markdown_table(&mut out, &table_rows);
                        table_rows.clear();
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML parse error: {e}")),
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(not(feature = "office-docs"))]
fn extract_docx(_bytes: &[u8]) -> anyhow::Result<Option<String>> {
    Ok(None)
}

#[cfg(feature = "office-docs")]
fn extract_pptx(bytes: &[u8]) -> anyhow::Result<Option<String>> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("not a valid .pptx (zip) file: {e}"))?;
    let mut slides: Vec<String> = archive
        .file_names()
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .map(|name| name.to_string())
        .collect();
    slides.sort_by_key(|name| slide_index(name));

    let mut out = String::new();
    for (idx, name) in slides.iter().enumerate() {
        let mut raw = Vec::new();
        {
            let mut file = match archive.by_name(name) {
                Ok(file) => file,
                Err(_) => continue,
            };
            if file.read_to_end(&mut raw).is_err() {
                continue;
            }
        }
        let xml = String::from_utf8_lossy(&raw);
        let text = xml_runs_to_text(&xml, &[b"a:t"], &[b"a:p"], &[], &[b"a:br"])?;
        out.push_str(&format!("--- Slide {} ---\n", idx + 1));
        out.push_str(text.trim_end());
        out.push_str("\n\n");
    }
    Ok(Some(out))
}

#[cfg(not(feature = "office-docs"))]
fn extract_pptx(_bytes: &[u8]) -> anyhow::Result<Option<String>> {
    Ok(None)
}

#[cfg(feature = "office-docs")]
fn slide_index(name: &str) -> u32 {
    name.trim_start_matches("ppt/slides/slide")
        .trim_end_matches(".xml")
        .parse()
        .unwrap_or(u32::MAX)
}

#[cfg(feature = "office-docs")]
fn extract_xlsx(bytes: &[u8]) -> anyhow::Result<Option<String>> {
    use calamine::{open_workbook_auto_from_rs, Reader};

    const MAX_ROWS: usize = 5000;
    const MAX_COLS: usize = 64;

    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut workbook = open_workbook_auto_from_rs(cursor)
        .map_err(|e| anyhow::anyhow!("failed to open spreadsheet: {e}"))?;

    let mut out = String::new();
    let names: Vec<String> = workbook.sheet_names().iter().map(|s| s.to_string()).collect();
    for name in names {
        let range = match workbook.worksheet_range(&name) {
            Ok(range) => range,
            Err(_) => continue,
        };
        if range.is_empty() {
            continue;
        }
        out.push_str(&format!("# Sheet: {name}\n\n"));
        let col_count = range
            .rows()
            .map(|r| r.len())
            .max()
            .unwrap_or(0)
            .min(MAX_COLS);
        if col_count == 0 {
            out.push('\n');
            continue;
        }
        let mut truncated_rows = false;
        for (idx, row) in range.rows().enumerate() {
            if idx > MAX_ROWS {
                truncated_rows = true;
                break;
            }
            let cells: Vec<String> = (0..col_count)
                .map(|c| {
                    row.get(c)
                        .map(|cell| markdown_table_cell(&cell.to_string()))
                        .unwrap_or_default()
                })
                .collect();
            out.push_str("| ");
            out.push_str(&cells.join(" | "));
            out.push_str(" |\n");
            if idx == 0 {
                out.push_str("| ");
                out.push_str(&vec!["---"; col_count].join(" | "));
                out.push_str(" |\n");
            }
        }
        if truncated_rows {
            out.push_str(&format!("\n... [more rows truncated at {MAX_ROWS}]\n"));
        }
        out.push('\n');
    }
    Ok(Some(out))
}

#[cfg(not(feature = "office-docs"))]
fn extract_xlsx(_bytes: &[u8]) -> anyhow::Result<Option<String>> {
    Ok(None)
}

#[cfg(feature = "office-docs")]
fn xml_runs_to_text(
    xml: &str,
    text_tags: &[&[u8]],
    para_tags: &[&[u8]],
    tab_tags: &[&[u8]],
    break_tags: &[&[u8]],
) -> anyhow::Result<String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut text_depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if text_tags.iter().any(|tag| *tag == e.name().as_ref()) {
                    text_depth += 1;
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let n = name.as_ref();
                if text_tags.iter().any(|tag| *tag == n) {
                    text_depth = text_depth.saturating_sub(1);
                }
                if para_tags.iter().any(|tag| *tag == n) {
                    out.push('\n');
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let n = name.as_ref();
                if tab_tags.iter().any(|tag| *tag == n) {
                    out.push('\t');
                }
                if break_tags.iter().any(|tag| *tag == n) {
                    out.push('\n');
                }
            }
            Ok(Event::Text(e)) => {
                if text_depth > 0 {
                    if let Ok(txt) = e.xml_content(quick_xml::XmlVersion::Implicit1_0) {
                        out.push_str(&txt);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML parse error: {e}")),
            _ => {}
        }
    }
    Ok(out)
}
