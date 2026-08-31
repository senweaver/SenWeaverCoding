// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {

    pub data: String,

    pub event: Option<String>,

    pub id: Option<String>,

    pub retry_ms: Option<u64>,
}

impl SseEvent {

    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

const MAX_SSE_LINE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SSE_DATA_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    pending: SseEvent,
    ready: std::collections::VecDeque<SseEvent>,
    saw_data: bool,
    skip_until_newline: bool,
    overflowed: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > MAX_SSE_LINE_BYTES
            && memchr::memchr2(b'\n', b'\r', &self.buf).is_none()
        {
            self.buf.clear();
            self.skip_until_newline = true;
            self.overflowed = true;
            tracing::warn!(
                target: "providers.sse",
                limit = MAX_SSE_LINE_BYTES,
                "SSE line exceeded size limit; dropping oversized buffer"
            );
            return;
        }
        self.parse_buffer();
    }

    pub fn next_event(&mut self) -> Option<SseEvent> {
        self.ready.pop_front()
    }

    pub fn finish(&mut self) {
        if !self.buf.is_empty() && !self.skip_until_newline {
            let buf = std::mem::take(&mut self.buf);
            let mut line_slice = buf.as_slice();
            if line_slice.last().copied() == Some(b'\r') {
                line_slice = &line_slice[..line_slice.len() - 1];
            }
            let line = String::from_utf8_lossy(line_slice);
            if !line.is_empty() {
                self.process_line(line.as_ref());
            }
        }
        self.skip_until_newline = false;
        if self.saw_data {
            self.dispatch_pending();
        }
        self.buf.clear();
    }

    fn parse_buffer(&mut self) {
        let mut cursor = 0usize;
        loop {
            let Some(rel) = memchr::memchr2(b'\n', b'\r', &self.buf[cursor..]) else {
                break;
            };
            let term = cursor + rel;
            let is_cr = self.buf[term] == b'\r';
            if is_cr && term + 1 >= self.buf.len() {
                break;
            }
            let after = if is_cr && self.buf[term + 1] == b'\n' {
                term + 2
            } else {
                term + 1
            };
            if self.skip_until_newline {
                self.skip_until_newline = false;
                cursor = after;
                continue;
            }
            let line_slice = &self.buf[cursor..term];
            let line: String = match std::str::from_utf8(line_slice) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    tracing::debug!(
                        target: "providers.sse",
                        "SSE line was not valid UTF-8; decoding lossily to avoid dropping content"
                    );
                    String::from_utf8_lossy(line_slice).into_owned()
                }
            };
            cursor = after;
            self.process_line(&line);
        }
        if cursor > 0 {
            self.buf.drain(..cursor);
        }
    }

    fn process_line(&mut self, line: &str) {

        if line.is_empty() {
            if self.saw_data {
                self.dispatch_pending();
            }
            return;
        }

        if line.starts_with(':') {
            return;
        }

        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "data" => {
                if self.pending.data.len() < MAX_SSE_DATA_BYTES {
                    if self.pending.data.is_empty() {
                        self.pending.data.push_str(value);
                    } else {
                        self.pending.data.push('\n');
                        self.pending.data.push_str(value);
                    }
                } else if !self.overflowed {
                    self.overflowed = true;
                    tracing::warn!(
                        target: "providers.sse",
                        limit = MAX_SSE_DATA_BYTES,
                        "SSE data field exceeded size limit; further data for this event is truncated"
                    );
                }
                self.saw_data = true;
            }
            "event" => self.pending.event = Some(value.to_string()),
            "id" => self.pending.id = Some(value.to_string()),
            "retry" => {
                if let Ok(n) = value.parse::<u64>() {
                    self.pending.retry_ms = Some(n);
                }
            }
            _ => {}
        }
    }

    fn dispatch_pending(&mut self) {
        let ev = std::mem::take(&mut self.pending);
        self.saw_data = false;
        self.ready.push_back(ev);
    }
}
