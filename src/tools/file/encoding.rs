// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use encoding_rs::Encoding;

const CANDIDATE_LABELS: [&str; 5] = ["GBK", "Big5", "Shift_JIS", "EUC-KR", "windows-1252"];

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

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
    if label.eq_ignore_ascii_case("UTF-16BE") {
        let mut out = Vec::with_capacity(2 + text.len() * 2);
        out.extend_from_slice(&[0xFE, 0xFF]);
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
