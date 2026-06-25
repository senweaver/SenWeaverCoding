// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Formula, Workbook};
use std::collections::HashMap;

pub struct XlsxSheet {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub merge_columns: Vec<usize>,
    pub freeze_header: bool,
    pub column_widths: Vec<f64>,
    pub number_formats: Vec<(usize, String)>,
}

pub fn value_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn header_format() -> Format {
    Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap()
        .set_border(FormatBorder::Thin)
        .set_background_color(Color::RGB(0x4472C4))
        .set_font_color(Color::RGB(0xFFFFFF))
}

fn body_format(num_format: Option<&str>) -> Format {
    let mut fmt = Format::new()
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap()
        .set_border(FormatBorder::Thin);
    if let Some(nf) = num_format {
        fmt = fmt.set_num_format(nf);
    }
    fmt
}

fn merge_format(num_format: Option<&str>) -> Format {
    let mut fmt = Format::new()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap()
        .set_border(FormatBorder::Thin);
    if let Some(nf) = num_format {
        fmt = fmt.set_num_format(nf);
    }
    fmt
}

fn write_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    value: &serde_json::Value,
    fmt: &Format,
) -> anyhow::Result<()> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                worksheet.write_with_format(row, col, f, fmt)?;
            } else {
                worksheet.write_with_format(row, col, value_to_text(value).as_str(), fmt)?;
            }
        }
        serde_json::Value::Bool(b) => {
            worksheet.write_with_format(row, col, *b, fmt)?;
        }
        serde_json::Value::Null => {
            worksheet.write_with_format(row, col, "", fmt)?;
        }
        serde_json::Value::String(s) => {
            if s.len() > 1 && s.starts_with('=') {
                worksheet.write_with_format(row, col, Formula::new(s.as_str()), fmt)?;
            } else {
                worksheet.write_with_format(row, col, s.as_str(), fmt)?;
            }
        }
        other => {
            worksheet.write_with_format(row, col, value_to_text(other).as_str(), fmt)?;
        }
    }
    Ok(())
}

pub fn write_workbook(sheets: &[XlsxSheet]) -> anyhow::Result<Vec<u8>> {
    if sheets.is_empty() {
        anyhow::bail!("xlsx export requires at least one sheet");
    }
    let mut workbook = Workbook::new();
    let header_fmt = header_format();

    for (sheet_idx, sheet) in sheets.iter().enumerate() {
        let worksheet = workbook.add_worksheet();
        let name = sanitize_sheet_name(&sheet.name, sheet_idx);
        worksheet.set_name(name.as_str())?;

        let col_count = sheet
            .columns
            .len()
            .max(sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0));
        if col_count == 0 {
            continue;
        }
        let col_count_u16 = u16::try_from(col_count.min(u16::MAX as usize)).unwrap_or(u16::MAX);

        let num_fmt_map: HashMap<usize, String> = sheet
            .number_formats
            .iter()
            .filter(|(_, s)| !s.trim().is_empty())
            .cloned()
            .collect();

        for c in 0..col_count_u16 {
            let header = sheet
                .columns
                .get(c as usize)
                .cloned()
                .unwrap_or_default();
            worksheet.write_with_format(0, c, header.as_str(), &header_fmt)?;
        }
        worksheet.set_row_height(0, 22)?;

        let merge_set: std::collections::HashSet<usize> =
            sheet.merge_columns.iter().copied().collect();

        for c in 0..col_count_u16 {
            let col_usize = c as usize;
            let num_fmt = num_fmt_map.get(&col_usize).map(String::as_str);
            let body_fmt = body_format(num_fmt);
            if merge_set.contains(&col_usize) {
                let merge_fmt = merge_format(num_fmt);
                write_merged_column(worksheet, c, sheet, &merge_fmt, &body_fmt)?;
            } else {
                for (r, row) in sheet.rows.iter().enumerate() {
                    let target_row = (r + 1) as u32;
                    let empty = serde_json::Value::Null;
                    let value = row.get(col_usize).unwrap_or(&empty);
                    write_cell(worksheet, target_row, c, value, &body_fmt)?;
                }
            }
        }

        if sheet.column_widths.is_empty() {
            worksheet.autofit();
        } else {
            for (c, width) in sheet.column_widths.iter().enumerate() {
                if c >= col_count {
                    break;
                }
                if *width > 0.0 {
                    worksheet.set_column_width(c as u16, *width)?;
                }
            }
        }

        if sheet.freeze_header {
            worksheet.set_freeze_panes(1, 0)?;
        }
    }

    let buffer = workbook.save_to_buffer()?;
    Ok(buffer)
}

fn write_merged_column(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    col: u16,
    sheet: &XlsxSheet,
    merge_fmt: &Format,
    body_fmt: &Format,
) -> anyhow::Result<()> {
    let col_usize = col as usize;
    let total = sheet.rows.len();
    let mut start = 0usize;
    while start < total {
        let empty = serde_json::Value::Null;
        let current = sheet.rows[start].get(col_usize).unwrap_or(&empty);
        let current_text = value_to_text(current);
        let mut end = start + 1;
        if !current_text.is_empty() {
            while end < total {
                let next = sheet.rows[end].get(col_usize).unwrap_or(&empty);
                if value_to_text(next) == current_text {
                    end += 1;
                } else {
                    break;
                }
            }
        }
        let first_row = (start + 1) as u32;
        if end - start > 1 {
            let last_row = end as u32;
            worksheet.merge_range(
                first_row,
                col,
                last_row,
                col,
                current_text.as_str(),
                merge_fmt,
            )?;
        } else {
            write_cell(worksheet, first_row, col, current, body_fmt)?;
        }
        start = end;
    }
    Ok(())
}

fn sanitize_sheet_name(raw: &str, idx: usize) -> String {
    let trimmed = raw.trim();
    let base = if trimmed.is_empty() {
        format!("Sheet{}", idx + 1)
    } else {
        trimmed.to_string()
    };
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '\\' | '/' | '?' | '*' | '[' | ']' | ':' => '_',
            other => other,
        })
        .collect();
    cleaned.chars().take(31).collect()
}
