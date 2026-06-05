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
            && memchr::memchr(b'\n', &self.buf).is_none()
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
        if self.saw_data {
            self.dispatch_pending();
        }
        self.buf.clear();
    }

    fn parse_buffer(&mut self) {

        while let Some(pos) = memchr::memchr(b'\n', &self.buf) {

            let line_bytes: Vec<u8> = self.buf.drain(..=pos).collect();
            if self.skip_until_newline {
                self.skip_until_newline = false;
                continue;
            }
            let line_len = line_bytes.len().saturating_sub(1);
            let mut line_slice = &line_bytes[..line_len];
            if line_slice.last().copied() == Some(b'\r') {
                line_slice = &line_slice[..line_slice.len() - 1];
            }
            let line = match std::str::from_utf8(line_slice) {
                Ok(s) => s,
                Err(_) => continue,
            };
            self.process_line(line);
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
