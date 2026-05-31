// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Default)]
pub struct ThinkTagSplitter {
    inside: bool,
    pending: String,
}

const OPENING_TAGS: &[&str] = &["<think>", "<thinking>", "<reasoning>"];
const CLOSING_TAGS: &[&str] = &["</think>", "</thinking>", "</reasoning>"];

impl ThinkTagSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inside(&self) -> bool {
        self.inside
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn split(&mut self, chunk: &str) -> (String, String) {
        let mut buf = std::mem::take(&mut self.pending);
        buf.push_str(chunk);

        let mut visible = String::new();
        let mut thinking = String::new();
        let mut cursor = 0usize;

        while cursor < buf.len() {
            let suffix = &buf[cursor..];
            let lt_pos = suffix.find('<');
            let plain_end = lt_pos.unwrap_or(suffix.len());

            if plain_end > 0 {
                let plain = &suffix[..plain_end];
                if self.inside {
                    thinking.push_str(plain);
                } else {
                    visible.push_str(plain);
                }
                cursor += plain_end;
            }

            if lt_pos.is_none() {
                break;
            }

            let tag_suffix = &buf[cursor..];
            let tags: &[&str] = if self.inside {
                CLOSING_TAGS
            } else {
                OPENING_TAGS
            };

            let mut matched_len: Option<usize> = None;
            for tag in tags {
                if tag_suffix.len() >= tag.len()
                    && tag_suffix[..tag.len()].eq_ignore_ascii_case(tag)
                {
                    matched_len = Some(tag.len());
                    break;
                }
            }

            if let Some(len) = matched_len {
                self.inside = !self.inside;
                cursor += len;
                continue;
            }

            let mut could_extend = false;
            for tag in tags {
                if tag.len() > tag_suffix.len()
                    && tag[..tag_suffix.len()].eq_ignore_ascii_case(tag_suffix)
                {
                    could_extend = true;
                    break;
                }
            }
            if could_extend {
                self.pending = tag_suffix.to_string();
                return (visible, thinking);
            }

            if self.inside {
                thinking.push('<');
            } else {
                visible.push('<');
            }
            cursor += 1;
        }

        (visible, thinking)
    }

    pub fn flush(&mut self) -> (String, String) {
        let pending = std::mem::take(&mut self.pending);
        let inside = self.inside;
        self.inside = false;
        if pending.is_empty() {
            return (String::new(), String::new());
        }
        if inside {
            (String::new(), pending)
        } else {
            (pending, String::new())
        }
    }
}
