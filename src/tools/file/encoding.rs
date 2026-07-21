// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use encoding_rs::Encoding;

const CANDIDATE_LABELS: [&str; 5] = ["GBK", "Big5", "Shift_JIS", "EUC-KR", "windows-1252"];

pub fn is_utf8_label(label: &str) -> bool {
    label.eq_ignore_ascii_case("UTF-8") || label.eq_ignore_ascii_case("utf8")
}

pub fn decode_best_effort(bytes: &[u8]) -> (String, &'static str) {
    if let Some((enc, _bom_len)) = Encoding::for_bom(bytes) {
        let (cow, _, _had_errors) = enc.decode(bytes);
        return (cow.into_owned(), enc.name());
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
    let enc = Encoding::for_label(label.as_bytes())?;
    let (cow, _, had_errors) = enc.encode(text);
    if had_errors {
        return None;
    }
    Some(cow.into_owned())
}
