// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::super::traits::{Tool, ToolResult};
use super::xlsx::{self, XlsxSheet};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct DocumentConvertTool {
    security: Arc<SecurityPolicy>,
}

impl DocumentConvertTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

enum RenderedBytes {
    InMemory(Vec<u8>),
    WrittenByRenderer,
}

#[async_trait]
impl Tool for DocumentConvertTool {
    fn name(&self) -> &str {
        "document_convert"
    }

    fn description(&self) -> &str {
        "Generate a real document file in a requested format from structured content you supply. \
         This is the writer half of an AI-driven document conversion: first read the source with \
         `file_read` (it extracts text/tables/headings from .docx/.pdf/.pptx/.xlsx), then map the \
         content yourself into the structure below and call this tool to materialise the target file. \
         Supported `target_format`: `xlsx` (styled Excel: bold header, borders, auto-fit columns, \
         frozen header row, vertical merge of repeated values in hierarchy columns, live Excel \
         formulas via cells starting with `=`, and per-column number formats), `csv`, \
         `docx` (Word), `md`, `html`, `pdf`. For tabular targets (xlsx/csv) provide `sheets` with `columns` \
         and `rows`; for prose targets (docx/md/html) provide `content_markdown` (or `sheets`, which \
         are rendered as tables). Example: convert a Word feature spec into an Excel sheet with \
         columns 序号/一级功能/二级功能/三级功能/功能描述, marking the hierarchy columns in \
         `merge_columns` so equal consecutive values are merged vertically. The output file is \
         written into the workspace and surfaced in the IDE."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "target_format": {
                    "type": "string",
                    "enum": ["xlsx", "csv", "docx", "md", "html", "pdf"],
                    "description": "Output file format."
                },
                "output_path": {
                    "type": "string",
                    "description": "Destination path (relative paths resolve from the workspace). Include the matching extension."
                },
                "font_path": {
                    "type": "string",
                    "description": "Optional path to a .ttf/.otf font, embedded when target_format=pdf. Required to render CJK/non-Latin text in PDF output."
                },
                "title": {
                    "type": "string",
                    "description": "Optional document title. For prose targets it is prepended as a top-level heading when `content_markdown` has none."
                },
                "source_path": {
                    "type": "string",
                    "description": "Optional path of the source document this was converted from (recorded for provenance only)."
                },
                "content_markdown": {
                    "type": "string",
                    "description": "Markdown body for prose targets (docx/md/html). Supports headings, paragraphs, lists, tables, blockquotes and fenced code blocks."
                },
                "sheets": {
                    "type": "array",
                    "description": "Tabular content. Required for xlsx/csv; for docx/md/html it is rendered as table(s) when `content_markdown` is absent.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Sheet/tab name (xlsx). Defaults to Sheet1, Sheet2, ..." },
                            "columns": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Header cells, left to right."
                            },
                            "rows": {
                                "type": "array",
                                "items": {
                                    "type": "array",
                                    "items": {}
                                },
                                "description": "Row data; each row is an array of cell values (string/number/boolean/null) aligned to `columns`. A string cell starting with `=` is written as an Excel formula (e.g. \"=SUM(B2:B9)\")."
                            },
                            "merge_columns": {
                                "type": "array",
                                "items": {},
                                "description": "Column indices (0-based) or column names whose consecutive equal values are merged vertically in xlsx (use for hierarchy columns)."
                            },
                            "freeze_header": {
                                "type": "boolean",
                                "description": "Freeze the header row in xlsx (default true)."
                            },
                            "column_widths": {
                                "type": "array",
                                "items": { "type": "number" },
                                "description": "Optional explicit column widths (xlsx). When omitted, columns auto-fit."
                            },
                            "number_formats": {
                                "type": "object",
                                "description": "Optional xlsx number formats keyed by column name or 0-based index, value is an Excel format code (e.g. {\"Revenue\": \"$#,##0.00\", \"3\": \"0.0%\"})."
                            }
                        },
                        "required": ["columns", "rows"]
                    }
                }
            },
            "required": ["target_format", "output_path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let target_format = args
            .get("target_format")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .ok_or_else(|| anyhow::anyhow!("Missing 'target_format' parameter"))?;
        let target_format = normalize_format(&target_format).ok_or_else(|| {
            anyhow::anyhow!(
                "Unsupported target_format '{target_format}'. Expected one of: xlsx, csv, docx, md, html."
            )
        })?;

        let path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing 'output_path' parameter"))?
            .to_string();

        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let content_markdown = args
            .get("content_markdown")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let sheets = parse_sheets(args.get("sheets"))?;

        match target_format {
            OutputFormat::Xlsx | OutputFormat::Csv => {
                if sheets.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "target_format '{}' requires non-empty 'sheets' (columns + rows).",
                            target_format.ext()
                        )),
                    });
                }
            }
            OutputFormat::Docx | OutputFormat::Md | OutputFormat::Html | OutputFormat::Pdf => {
                let has_prose = content_markdown
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                if !has_prose && sheets.is_empty() {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "target_format '{}' requires 'content_markdown' or 'sheets'.",
                            target_format.ext()
                        )),
                    });
                }
            }
        }

        if !self.security.can_act() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Action blocked: autonomy is read-only".into()),
            });
        }
        if self.security.is_rate_limited() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: too many actions in the last hour".into()),
            });
        }
        if !self.security.is_path_allowed(&path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path}")),
            });
        }

        let full_path = self.security.resolve_tool_path(&path);
        let Some(parent) = full_path.parent() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid path: missing parent directory".into()),
            });
        };
        tokio::fs::create_dir_all(parent).await?;
        let resolved_parent = match tokio::fs::canonicalize(parent).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to resolve file path: {e}")),
                });
            }
        };
        if !self.security.is_resolved_path_allowed(&resolved_parent) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    self.security
                        .resolved_path_violation_message(&resolved_parent),
                ),
            });
        }
        if !crate::security::sandbox_allows_path(&resolved_parent) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Sandbox policy denies write to {}",
                    resolved_parent.display()
                )),
            });
        }
        let Some(file_name) = full_path.file_name() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Invalid path: missing file name".into()),
            });
        };
        let resolved_target = resolved_parent.join(file_name);
        if self.security.is_runtime_config_path(&resolved_target) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    self.security
                        .runtime_config_violation_message(&resolved_target),
                ),
            });
        }
        if let Ok(meta) = tokio::fs::symlink_metadata(&resolved_target).await {
            if meta.file_type().is_symlink() {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Refusing to write through symlink: {}",
                        resolved_target.display()
                    )),
                });
            }
        }
        if !self.security.record_action() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Rate limit exceeded: action budget exhausted".into()),
            });
        }

        let before_bytes = tokio::fs::read(&resolved_target).await.ok();

        let font_bytes = if matches!(target_format, OutputFormat::Pdf) {
            match args
                .get("font_path")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(fp) => match super::common::resolve_read_source(&self.security, fp) {
                    Ok(p) => tokio::fs::read(&p).await.ok(),
                    Err(e) => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("font_path error: {e}")),
                        });
                    }
                },
                None => None,
            }
        } else {
            None
        };

        let (rendered, render_note) = match render_output(
            target_format,
            &sheets,
            content_markdown.as_deref(),
            title.as_deref(),
            &resolved_target,
            font_bytes,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to render {}: {e}", target_format.ext())),
                });
            }
        };

        let final_bytes = match rendered {
            RenderedBytes::InMemory(bytes) => {
                if let Err(e) = tokio::fs::write(&resolved_target, &bytes).await {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to write file: {e}")),
                    });
                }
                bytes
            }
            RenderedBytes::WrittenByRenderer => {
                tokio::fs::read(&resolved_target).await.unwrap_or_default()
            }
        };

        crate::session::record_write_for_current_session(&resolved_target);
        crate::agent::file_edit_emitter::emit_file_edit(
            &resolved_target,
            before_bytes.as_deref(),
            Some(&final_bytes),
            None,
        )
        .await;

        let summary = build_summary(target_format, &sheets, &content_markdown);
        let note = render_note
            .map(|n| format!(" Note: {n}"))
            .unwrap_or_default();
        Ok(ToolResult {
            success: true,
            output: format!(
                "Wrote {} bytes to {path} ({} document).{summary}{note}",
                final_bytes.len(),
                target_format.ext()
            ),
            error: None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Xlsx,
    Csv,
    Docx,
    Md,
    Html,
    Pdf,
}

impl OutputFormat {
    fn ext(&self) -> &'static str {
        match self {
            OutputFormat::Xlsx => "xlsx",
            OutputFormat::Csv => "csv",
            OutputFormat::Docx => "docx",
            OutputFormat::Md => "md",
            OutputFormat::Html => "html",
            OutputFormat::Pdf => "pdf",
        }
    }
}

fn normalize_format(raw: &str) -> Option<OutputFormat> {
    match raw {
        "xlsx" | "excel" | "xls" => Some(OutputFormat::Xlsx),
        "csv" => Some(OutputFormat::Csv),
        "docx" | "word" | "doc" => Some(OutputFormat::Docx),
        "md" | "markdown" => Some(OutputFormat::Md),
        "html" | "htm" => Some(OutputFormat::Html),
        "pdf" => Some(OutputFormat::Pdf),
        _ => None,
    }
}

fn parse_sheets(value: Option<&serde_json::Value>) -> anyhow::Result<Vec<XlsxSheet>> {
    let Some(arr) = value.and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut sheets = Vec::with_capacity(arr.len());
    for (idx, raw) in arr.iter().enumerate() {
        let obj = raw
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("sheets[{idx}] must be an object"))?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Sheet{}", idx + 1));
        let columns: Vec<String> = obj
            .get("columns")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().map(xlsx::value_to_text).collect())
            .unwrap_or_default();
        let rows: Vec<Vec<serde_json::Value>> = obj
            .get("rows")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .map(|row| {
                        row.as_array()
                            .map(|cells| cells.to_vec())
                            .unwrap_or_else(|| vec![row.clone()])
                    })
                    .collect()
            })
            .unwrap_or_default();
        let merge_columns = resolve_merge_columns(obj.get("merge_columns"), &columns);
        let freeze_header = obj
            .get("freeze_header")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let column_widths: Vec<f64> = obj
            .get("column_widths")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_f64()).collect())
            .unwrap_or_default();
        let number_formats = resolve_number_formats(obj.get("number_formats"), &columns);
        sheets.push(XlsxSheet {
            name,
            columns,
            rows,
            merge_columns,
            freeze_header,
            column_widths,
            number_formats,
        });
    }
    Ok(sheets)
}

fn resolve_column_key(key: &str, columns: &[String]) -> Option<usize> {
    if let Ok(i) = key.trim().parse::<usize>() {
        return Some(i);
    }
    columns.iter().position(|c| c == key)
}

fn resolve_number_formats(
    value: Option<&serde_json::Value>,
    columns: &[String],
) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    match value {
        Some(serde_json::Value::Object(map)) => {
            for (k, v) in map {
                if let (Some(idx), Some(fmt)) = (
                    resolve_column_key(k, columns),
                    v.as_str().filter(|s| !s.trim().is_empty()),
                ) {
                    out.push((idx, fmt.to_string()));
                }
            }
        }
        Some(serde_json::Value::Array(arr)) => {
            for entry in arr {
                if let Some(pair) = entry.as_array() {
                    if pair.len() == 2 {
                        let idx = pair[0]
                            .as_u64()
                            .map(|n| n as usize)
                            .or_else(|| pair[0].as_str().and_then(|s| resolve_column_key(s, columns)));
                        let fmt = pair[1].as_str().filter(|s| !s.trim().is_empty());
                        if let (Some(idx), Some(fmt)) = (idx, fmt) {
                            out.push((idx, fmt.to_string()));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn resolve_merge_columns(value: Option<&serde_json::Value>, columns: &[String]) -> Vec<usize> {
    let Some(arr) = value.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in arr {
        if let Some(i) = entry.as_u64() {
            out.push(i as usize);
        } else if let Some(name) = entry.as_str() {
            if let Some(idx) = columns.iter().position(|c| c == name) {
                out.push(idx);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

async fn render_output(
    format: OutputFormat,
    sheets: &[XlsxSheet],
    content_markdown: Option<&str>,
    title: Option<&str>,
    resolved_target: &std::path::Path,
    font_bytes: Option<Vec<u8>>,
) -> anyhow::Result<(RenderedBytes, Option<String>)> {
    match format {
        OutputFormat::Xlsx => {
            let owned: Vec<XlsxSheet> = sheets.iter().map(clone_sheet).collect();
            let bytes = tokio::task::spawn_blocking(move || xlsx::write_workbook(&owned))
                .await
                .map_err(|e| anyhow::anyhow!("xlsx render task failed: {e}"))??;
            Ok((RenderedBytes::InMemory(bytes), None))
        }
        OutputFormat::Csv => {
            let csv = sheet_to_csv(&sheets[0]);
            let mut bytes = Vec::with_capacity(csv.len() + 3);
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            bytes.extend_from_slice(csv.as_bytes());
            Ok((RenderedBytes::InMemory(bytes), None))
        }
        OutputFormat::Md => {
            let md = compose_markdown(content_markdown, sheets, title);
            Ok((RenderedBytes::InMemory(md.into_bytes()), None))
        }
        OutputFormat::Html => {
            let md = compose_markdown(content_markdown, sheets, title);
            let html = markdown_to_html_document(&md, title);
            Ok((RenderedBytes::InMemory(html.into_bytes()), None))
        }
        OutputFormat::Docx => {
            let md = compose_markdown(content_markdown, sheets, title);
            render_docx(md, resolved_target).await?;
            Ok((RenderedBytes::WrittenByRenderer, None))
        }
        OutputFormat::Pdf => {
            let md = compose_markdown(content_markdown, sheets, title);
            let title_owned = title.map(str::to_string);
            let result = tokio::task::spawn_blocking(move || {
                super::pdf_render::render_markdown_pdf(&md, title_owned.as_deref(), font_bytes)
            })
            .await
            .map_err(|e| anyhow::anyhow!("pdf render task failed: {e}"))??;
            Ok((RenderedBytes::InMemory(result.bytes), result.warning))
        }
    }
}

#[cfg(feature = "tool-curator")]
async fn render_docx(markdown: String, resolved_target: &std::path::Path) -> anyhow::Result<()> {
    let target = resolved_target.to_path_buf();
    tokio::task::spawn_blocking(move || {
        crate::tools::curator::docx::render_docx(
            &markdown,
            crate::tools::curator::CuratorTemplateKind::PaperImrad,
            &target,
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("docx render task failed: {e}"))??;
    Ok(())
}

#[cfg(not(feature = "tool-curator"))]
async fn render_docx(_markdown: String, _resolved_target: &std::path::Path) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "DOCX output requires the `tool-curator` feature (the DOCX renderer). \
         Rebuild with it enabled, or choose target_format md/html/xlsx/csv."
    ))
}

fn clone_sheet(sheet: &XlsxSheet) -> XlsxSheet {
    XlsxSheet {
        name: sheet.name.clone(),
        columns: sheet.columns.clone(),
        rows: sheet.rows.clone(),
        merge_columns: sheet.merge_columns.clone(),
        freeze_header: sheet.freeze_header,
        column_widths: sheet.column_widths.clone(),
        number_formats: sheet.number_formats.clone(),
    }
}

fn compose_markdown(
    content_markdown: Option<&str>,
    sheets: &[XlsxSheet],
    title: Option<&str>,
) -> String {
    if let Some(body) = content_markdown.filter(|s| !s.trim().is_empty()) {
        let has_h1 = body.lines().any(|l| l.trim_start().starts_with("# "));
        if let Some(t) = title.filter(|_| !has_h1) {
            return format!("# {t}\n\n{body}");
        }
        return body.to_string();
    }
    sheets_to_markdown(sheets, title)
}

fn sheets_to_markdown(sheets: &[XlsxSheet], title: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(t) = title {
        out.push_str(&format!("# {t}\n\n"));
    }
    for sheet in sheets {
        if sheets.len() > 1 || !sheet.name.is_empty() {
            out.push_str(&format!("## {}\n\n", sheet.name));
        }
        out.push_str(&sheet_to_markdown_table(sheet));
        out.push('\n');
    }
    out
}

fn sheet_to_markdown_table(sheet: &XlsxSheet) -> String {
    let col_count = sheet
        .columns
        .len()
        .max(sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if col_count == 0 {
        return String::new();
    }
    let mut out = String::new();
    let header: Vec<String> = (0..col_count)
        .map(|c| {
            md_escape_cell(
                sheet
                    .columns
                    .get(c)
                    .cloned()
                    .unwrap_or_default()
                    .as_str(),
            )
        })
        .collect();
    out.push_str(&format!("| {} |\n", header.join(" | ")));
    out.push_str(&format!(
        "| {} |\n",
        vec!["---"; col_count].join(" | ")
    ));
    for row in &sheet.rows {
        let cells: Vec<String> = (0..col_count)
            .map(|c| {
                let empty = serde_json::Value::Null;
                md_escape_cell(&xlsx::value_to_text(row.get(c).unwrap_or(&empty)))
            })
            .collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

fn md_escape_cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "<br>")
}

fn sheet_to_csv(sheet: &XlsxSheet) -> String {
    let col_count = sheet
        .columns
        .len()
        .max(sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0));
    let mut out = String::new();
    if col_count > 0 {
        let header: Vec<String> = (0..col_count)
            .map(|c| csv_field(sheet.columns.get(c).map(String::as_str).unwrap_or("")))
            .collect();
        out.push_str(&header.join(","));
        out.push_str("\r\n");
    }
    for row in &sheet.rows {
        let cells: Vec<String> = (0..col_count)
            .map(|c| {
                let empty = serde_json::Value::Null;
                csv_field(&xlsx::value_to_text(row.get(c).unwrap_or(&empty)))
            })
            .collect();
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    }
    out
}

fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn build_summary(
    format: OutputFormat,
    sheets: &[XlsxSheet],
    content_markdown: &Option<String>,
) -> String {
    match format {
        OutputFormat::Csv => {
            let rows = sheets.first().map(|s| s.rows.len()).unwrap_or(0);
            if sheets.len() > 1 {
                format!(
                    " Wrote first sheet ({rows} rows); CSV is single-sheet, so the other {} sheet(s) were not exported  -  use target_format xlsx to keep every sheet.",
                    sheets.len() - 1
                )
            } else {
                format!(" Rows: {rows}.")
            }
        }
        OutputFormat::Xlsx => {
            let total_rows: usize = sheets.iter().map(|s| s.rows.len()).sum();
            format!(" Sheets: {}, rows: {}.", sheets.len(), total_rows)
        }
        _ => {
            if content_markdown
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            {
                String::new()
            } else {
                let total_rows: usize = sheets.iter().map(|s| s.rows.len()).sum();
                format!(" Rendered {} table row(s).", total_rows)
            }
        }
    }
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn markdown_to_html_document(markdown: &str, title: Option<&str>) -> String {
    let body = markdown_to_html_body(markdown);
    let doc_title = html_escape(title.unwrap_or("Document"));
    format!(
        "<!DOCTYPE html>\n<html lang=\"zh\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{doc_title}</title>\n<style>\n\
         body{{font-family:-apple-system,Segoe UI,Roboto,\"Microsoft YaHei\",sans-serif;\
         line-height:1.6;max-width:920px;margin:2rem auto;padding:0 1rem;color:#1a1a1a;}}\n\
         h1,h2,h3,h4{{line-height:1.25;}}\n\
         table{{border-collapse:collapse;width:100%;margin:1rem 0;}}\n\
         th,td{{border:1px solid #c0c0c8;padding:6px 10px;text-align:left;vertical-align:top;}}\n\
         th{{background:#4472C4;color:#fff;}}\n\
         tr:nth-child(even) td{{background:#f6f6f8;}}\n\
         code{{background:#f4f4f5;padding:0.1em 0.3em;border-radius:3px;}}\n\
         pre{{background:#f4f4f5;padding:0.8rem;border-radius:6px;overflow:auto;}}\n\
         blockquote{{border-left:4px solid #c0c0c8;margin:1rem 0;padding:0.2rem 1rem;color:#555;}}\n\
         </style>\n</head>\n<body>\n{body}</body>\n</html>\n"
    )
}

fn markdown_to_html_body(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("```") {
            let _lang = rest.trim();
            let mut code = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push_str(&html_escape(lines[i]));
                code.push('\n');
                i += 1;
            }
            if i < lines.len() {
                i += 1;
            }
            out.push_str(&format!("<pre><code>{code}</code></pre>\n"));
            continue;
        }

        let mut heading = None;
        for level in (1..=6).rev() {
            let prefix = format!("{} ", "#".repeat(level));
            if let Some(rest) = trimmed.strip_prefix(&prefix) {
                heading = Some((level, rest.trim().to_string()));
                break;
            }
        }
        if let Some((level, text)) = heading {
            out.push_str(&format!(
                "<h{level}>{}</h{level}>\n",
                inline_to_html(&text)
            ));
            i += 1;
            continue;
        }

        if is_table_row(line) && i + 1 < lines.len() && is_table_separator(lines[i + 1]) {
            let header = split_table_row(line);
            i += 2;
            let mut rows: Vec<Vec<String>> = Vec::new();
            while i < lines.len() && is_table_row(lines[i]) {
                rows.push(split_table_row(lines[i]));
                i += 1;
            }
            out.push_str("<table>\n<thead><tr>");
            for cell in &header {
                out.push_str(&format!("<th>{}</th>", inline_to_html(cell)));
            }
            out.push_str("</tr></thead>\n<tbody>\n");
            for row in &rows {
                out.push_str("<tr>");
                for cell in row {
                    out.push_str(&format!("<td>{}</td>", inline_to_html(cell)));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</tbody>\n</table>\n");
            continue;
        }

        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            out.push_str("<ul>\n");
            while i < lines.len() {
                let cur = lines[i].trim_start();
                if let Some(rest) = cur.strip_prefix("- ").or_else(|| cur.strip_prefix("* ")) {
                    out.push_str(&format!("<li>{}</li>\n", inline_to_html(rest.trim())));
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("</ul>\n");
            continue;
        }

        if let Some((num, _)) = trimmed.split_once(". ") {
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                out.push_str("<ol>\n");
                while i < lines.len() {
                    let cur = lines[i].trim_start();
                    if let Some((n, rest)) = cur.split_once(". ") {
                        if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                            out.push_str(&format!("<li>{}</li>\n", inline_to_html(rest.trim())));
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                out.push_str("</ol>\n");
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push_str(&format!(
                "<blockquote>{}</blockquote>\n",
                inline_to_html(rest.trim())
            ));
            i += 1;
            continue;
        }

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        out.push_str(&format!("<p>{}</p>\n", inline_to_html(trimmed)));
        i += 1;
    }
    out
}

fn inline_to_html(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut buf = String::new();
    let mut bold = false;
    let mut italic = false;
    let mut code = false;
    let mut i = 0usize;
    macro_rules! flush {
        () => {{
            if !buf.is_empty() {
                out.push_str(&html_escape(&buf));
                buf.clear();
            }
        }};
    }
    while i < chars.len() {
        let ch = chars[i];
        if ch == '`' {
            flush!();
            if code {
                out.push_str("</code>");
            } else {
                out.push_str("<code>");
            }
            code = !code;
            i += 1;
            continue;
        }
        if !code && ch == '[' {
            if let Some((label, url, next)) = parse_inline_link(&chars, i) {
                flush!();
                out.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    html_escape(&url),
                    inline_to_html(&label)
                ));
                i = next;
                continue;
            }
        }
        if !code && ch == '*' && chars.get(i + 1) == Some(&'*') {
            flush!();
            if bold {
                out.push_str("</strong>");
            } else {
                out.push_str("<strong>");
            }
            bold = !bold;
            i += 2;
            continue;
        }
        if !code && (ch == '*' || ch == '_') {
            flush!();
            if italic {
                out.push_str("</em>");
            } else {
                out.push_str("<em>");
            }
            italic = !italic;
            i += 1;
            continue;
        }
        buf.push(ch);
        i += 1;
    }
    flush!();
    if code {
        out.push_str("</code>");
    }
    if italic {
        out.push_str("</em>");
    }
    if bold {
        out.push_str("</strong>");
    }
    out
}

fn parse_inline_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if chars.get(start) != Some(&'[') {
        return None;
    }
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
    let mut url = String::new();
    while k < chars.len() && chars[k] != ')' {
        url.push(chars[k]);
        k += 1;
    }
    if k >= chars.len() || label.is_empty() || url.trim().is_empty() {
        return None;
    }
    Some((label, url.trim().to_string(), k + 1))
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
    let inner = t.trim_matches('|');
    inner
        .split('|')
        .map(|c| c.trim())
        .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':' || c == ' '))
}

fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    trimmed.split('|').map(|c| c.trim().to_string()).collect()
}
