// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub fn split_message(text: &str, max_chars: usize, max_chunks: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() && chunks.len() < max_chunks {
        let is_last_slot = chunks.len() + 1 == max_chunks;

        if remaining.len() <= max_chars {
            chunks.push(remaining.to_string());
            remaining = "";
            continue;
        }

        let split_at = if is_last_slot {
            let indicator = "… (truncated)";
            let budget = max_chars.saturating_sub(indicator.len());
            let boundary = find_word_boundary(remaining, budget);
            chunks.push(format!("{}{indicator}", &remaining[..boundary]));
            remaining = "";
            continue;
        } else {
            find_word_boundary(remaining, max_chars)
        };

        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }

    if chunks.is_empty() {
        chunks.push(crate::util::truncate_str_bytes(text, max_chars).to_string());
    }

    chunks
}

fn find_word_boundary(text: &str, max_bytes: usize) -> usize {
    if max_bytes >= text.len() {
        return text.len();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(ws_pos) = text[..end].rfind(char::is_whitespace) {
        if ws_pos > end / 2 {
            return ws_pos;
        }
    }
    end
}
