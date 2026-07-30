// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use encoding_rs::Encoding;

const CANDIDATE_LABELS: [&str; 5] = ["GBK", "Big5", "Shift_JIS", "EUC-KR", "windows-1252"];

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

pub fn is_probably_binary(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return false;
    }
    if decode_utf16_without_bom(bytes).is_some() {
        return false;
    }
    if bytes.contains(&0) {
        return true;
    }
    let sample_len = bytes.len().min(8192);
    if sample_len == 0 {
        return false;
    }
    let controls = bytes[..sample_len]
        .iter()
        .filter(|byte| matches!(byte, 0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F))
        .count();
    controls.saturating_mul(100) > sample_len.saturating_mul(5)
}

fn decode_utf16_without_bom(bytes: &[u8]) -> Option<(String, &'static str)> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let pairs = bytes.len() / 2;
    let even_nuls = bytes.iter().step_by(2).filter(|byte| **byte == 0).count();
    let odd_nuls = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    let little_endian = odd_nuls.saturating_mul(100) >= pairs.saturating_mul(30)
        && even_nuls.saturating_mul(100) <= pairs.saturating_mul(5);
    let big_endian = even_nuls.saturating_mul(100) >= pairs.saturating_mul(30)
        && odd_nuls.saturating_mul(100) <= pairs.saturating_mul(5);
    if !little_endian && !big_endian {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    let text = String::from_utf16(&units).ok()?;
    Some((
        text,
        if little_endian {
            "UTF-16LE-NOBOM"
        } else {
            "UTF-16BE-NOBOM"
        },
    ))
}

pub fn is_utf8_label(label: &str) -> bool {
    label.eq_ignore_ascii_case("UTF-8") || label.eq_ignore_ascii_case("utf8")
}

pub fn decode_best_effort(bytes: &[u8]) -> (String, &'static str) {
    if let Some((enc, _bom_len)) = Encoding::for_bom(bytes) {
        let (cow, _, _had_errors) = enc.decode(bytes);
        let label = if enc == encoding_rs::UTF_8 {
            "UTF-8-BOM"
        } else {
            enc.name()
        };
        return (cow.into_owned(), label);
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return (text.to_string(), "UTF-8");
    }
    if let Some(decoded) = decode_utf16_without_bom(bytes) {
        return decoded;
    }

    for label in CANDIDATE_LABELS {
        let Some(enc) = Encoding::for_label(label.as_bytes()) else {
            continue;
        };
        let (cow, had_errors) = enc.decode_without_bom_handling(bytes);
        if !had_errors {
            return (cow.into_owned(), enc.name());
        }
    }

    (String::from_utf8_lossy(bytes).into_owned(), "UTF-8")
}

pub fn decode_for_edit(bytes: &[u8]) -> std::io::Result<(String, &'static str)> {
    if let Some((enc, _)) = Encoding::for_bom(bytes) {
        let (text, _, had_errors) = enc.decode(bytes);
        if had_errors {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "text encoding contains invalid byte sequences",
            ));
        }
        let label = if enc == encoding_rs::UTF_8 {
            "UTF-8-BOM"
        } else {
            enc.name()
        };
        return Ok((text.into_owned(), label));
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok((text.to_string(), "UTF-8"));
    }
    if let Some(decoded) = decode_utf16_without_bom(bytes) {
        return Ok(decoded);
    }
    let mut matches = Vec::new();
    for label in CANDIDATE_LABELS {
        let Some(encoding) = Encoding::for_label(label.as_bytes()) else {
            continue;
        };
        let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
        if had_errors {
            continue;
        }
        let (encoded, _, encode_errors) = encoding.encode(&text);
        if !encode_errors && encoded.as_ref() == bytes {
            matches.push((text.into_owned(), encoding.name()));
        }
    }
    matches.dedup_by(|left, right| left.0 == right.0);
    if matches.len() == 1 {
        return Ok(matches.remove(0));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "file encoding is ambiguous; refusing to rewrite without an explicit encoding",
    ))
}

pub fn detect_label(bytes: &[u8]) -> &'static str {
    let (_text, label) = decode_best_effort(bytes);
    label
}

pub fn encode_with_label(label: &str, text: &str) -> Option<Vec<u8>> {
    if is_utf8_label(label) {
        return Some(text.as_bytes().to_vec());
    }
    if label.eq_ignore_ascii_case("UTF-8-BOM") {
        let mut out = Vec::with_capacity(UTF8_BOM.len() + text.len());
        out.extend_from_slice(&UTF8_BOM);
        out.extend_from_slice(text.as_bytes());
        return Some(out);
    }
    if label.eq_ignore_ascii_case("UTF-16LE") {
        let mut out = Vec::with_capacity(2 + text.len() * 2);
        out.extend_from_slice(&[0xFF, 0xFE]);
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        return Some(out);
    }
    if label.eq_ignore_ascii_case("UTF-16LE-NOBOM") {
        let mut out = Vec::with_capacity(text.len() * 2);
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        return Some(out);
    }
    if label.eq_ignore_ascii_case("UTF-16BE") {
        let mut out = Vec::with_capacity(2 + text.len() * 2);
        out.extend_from_slice(&[0xFE, 0xFF]);
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_be_bytes());
        }
        return Some(out);
    }
    if label.eq_ignore_ascii_case("UTF-16BE-NOBOM") {
        let mut out = Vec::with_capacity(text.len() * 2);
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_be_bytes());
        }
        return Some(out);
    }
    let enc = Encoding::for_label(label.as_bytes())?;
    let (cow, _, had_errors) = enc.encode(text);
    if had_errors {
        return None;
    }
    Some(cow.into_owned())
}
