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
    Ok(Some(text))
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
    let text = xml_runs_to_text(
        &xml,
        &[b"w:t"],
        &[b"w:p"],
        &[b"w:tab"],
        &[b"w:br", b"w:cr"],
    )?;
    Ok(Some(text))
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
        out.push_str(&format!("# Sheet: {name}\n"));
        for (idx, row) in range.rows().enumerate() {
            if idx >= MAX_ROWS {
                out.push_str("... [more rows truncated]\n");
                break;
            }
            let line = row
                .iter()
                .take(MAX_COLS)
                .map(|cell| cell.to_string())
                .collect::<Vec<_>>()
                .join("\t");
            out.push_str(line.trim_end());
            out.push('\n');
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
