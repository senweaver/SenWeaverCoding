// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{BTreeMap, BTreeSet};

pub struct EmbeddedPdfFont {
    pub parsed: printpdf::ParsedFont,
    advance_em: BTreeMap<char, f32>,
    pub font_name: String,
}

impl EmbeddedPdfFont {
    pub fn covers(&self, ch: char) -> bool {
        self.advance_em.contains_key(&ch)
    }

    pub fn advance_em(&self, ch: char) -> Option<f32> {
        self.advance_em.get(&ch).copied()
    }
}

pub fn face_count(bytes: &[u8]) -> u32 {
    ttf_parser::fonts_in_collection(bytes).unwrap_or(1).max(1)
}

pub fn best_face_index(bytes: &[u8], required: &BTreeSet<char>, good: f32) -> Option<(u32, f32)> {
    let count = face_count(bytes).min(8);
    let mut best: Option<(u32, f32)> = None;
    for idx in 0..count {
        if let Some(ratio) = coverage_ratio(bytes, idx, required) {
            if best.map(|(_, r)| ratio > r).unwrap_or(true) {
                best = Some((idx, ratio));
            }
            if ratio >= good {
                return Some((idx, ratio));
            }
        }
    }
    best
}

pub fn coverage_ratio(bytes: &[u8], index: u32, chars: &BTreeSet<char>) -> Option<f32> {
    let face = ttf_parser::Face::parse(bytes, index).ok()?;
    if face.tables().cff2.is_some() {
        return None;
    }
    let checkable: Vec<char> = chars
        .iter()
        .copied()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect();
    if checkable.is_empty() {
        return Some(1.0);
    }
    let covered = checkable
        .iter()
        .filter(|c| face.glyph_index(**c).is_some())
        .count();
    Some(covered as f32 / checkable.len() as f32)
}

pub fn load_embedded_font(
    bytes: &[u8],
    index: u32,
    used_chars: &BTreeSet<char>,
) -> anyhow::Result<EmbeddedPdfFont> {
    let face = ttf_parser::Face::parse(bytes, index)
        .map_err(|e| anyhow::anyhow!("failed to parse font file: {e}"))?;
    if face.tables().cff2.is_some() {
        return Err(anyhow::anyhow!(
            "CFF2 (variable-outline) fonts are not supported for PDF embedding; use a .ttf/.otf/.ttc font"
        ));
    }
    let units_per_em = face.units_per_em();
    if units_per_em == 0 {
        return Err(anyhow::anyhow!("font reports zero units_per_em"));
    }

    let mut charset: BTreeSet<char> = used_chars.clone();
    charset.insert('?');
    charset.insert(' ');

    let mut char_to_old_gid: Vec<(char, u16)> = Vec::with_capacity(charset.len());
    for &ch in &charset {
        if let Some(gid) = face.glyph_index(ch) {
            char_to_old_gid.push((ch, gid.0));
        }
    }
    if char_to_old_gid.is_empty() {
        return Err(anyhow::anyhow!(
            "font does not cover any of the required characters"
        ));
    }

    let mut remapper = subsetter::GlyphRemapper::new();
    let mut old_to_new: BTreeMap<u16, u16> = BTreeMap::new();
    for &(_, old_gid) in &char_to_old_gid {
        let new_gid = remapper.remap(old_gid);
        old_to_new.insert(old_gid, new_gid);
    }

    let subset_sfnt = subsetter::subset(bytes, index, &remapper)
        .map_err(|e| anyhow::anyhow!("font subsetting failed: {e}"))?;

    let is_cff = face.tables().cff.is_some();
    let (final_bytes, font_type) = if is_cff {
        let cff = extract_sfnt_table(&subset_sfnt, b"CFF ").ok_or_else(|| {
            anyhow::anyhow!("subset font is missing the CFF table required for PDF embedding")
        })?;
        (cff, printpdf::FontType::OpenTypeCFF(()))
    } else {
        (subset_sfnt, printpdf::FontType::TrueType)
    };

    let mut codepoint_to_glyph: BTreeMap<u32, u16> = BTreeMap::new();
    let mut glyph_widths: BTreeMap<u16, u16> = BTreeMap::new();
    let mut advance_em: BTreeMap<char, f32> = BTreeMap::new();
    for &(ch, old_gid) in &char_to_old_gid {
        let Some(&new_gid) = old_to_new.get(&old_gid) else {
            continue;
        };
        let advance = face
            .glyph_hor_advance(ttf_parser::GlyphId(old_gid))
            .unwrap_or(units_per_em / 2);
        codepoint_to_glyph.insert(ch as u32, new_gid);
        glyph_widths.insert(new_gid, advance);
        advance_em.insert(ch, advance as f32 / units_per_em as f32);
    }

    let bbox = face.global_bounding_box();
    let font_name = sanitized_font_name(&face);

    let parsed = printpdf::ParsedFont {
        original_bytes: final_bytes,
        font_index: 0,
        font_name: Some(font_name.clone()),
        codepoint_to_glyph,
        glyph_widths,
        units_per_em,
        font_metrics: printpdf::FontMetrics {
            ascent: face.ascender(),
            descent: face.descender(),
        },
        font_type,
        pdf_font_metrics: printpdf::PdfFontMetricsStub {
            units_per_em,
            x_min: bbox.x_min,
            y_min: bbox.y_min,
            x_max: bbox.x_max,
            y_max: bbox.y_max,
        },
    };

    Ok(EmbeddedPdfFont {
        parsed,
        advance_em,
        font_name,
    })
}

fn sanitized_font_name(face: &ttf_parser::Face) -> String {
    let raw = face
        .names()
        .into_iter()
        .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME && n.is_unicode())
        .and_then(|n| n.to_string())
        .or_else(|| {
            face.names()
                .into_iter()
                .find(|n| n.name_id == ttf_parser::name_id::FULL_NAME && n.is_unicode())
                .and_then(|n| n.to_string())
        })
        .unwrap_or_else(|| "EmbeddedFont".to_string());
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if cleaned.is_empty() {
        "EmbeddedFont".to_string()
    } else {
        cleaned
    }
}

fn extract_sfnt_table(data: &[u8], tag: &[u8; 4]) -> Option<Vec<u8>> {
    if data.len() < 12 {
        return None;
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    let dir_end = 12usize.checked_add(num_tables.checked_mul(16)?)?;
    if data.len() < dir_end {
        return None;
    }
    for i in 0..num_tables {
        let off = 12 + i * 16;
        let entry = &data[off..off + 16];
        if &entry[0..4] == tag {
            let start = u32::from_be_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
            let len = u32::from_be_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
            let end = start.checked_add(len)?;
            if end <= data.len() {
                return Some(data[start..end].to_vec());
            }
            return None;
        }
    }
    None
}
