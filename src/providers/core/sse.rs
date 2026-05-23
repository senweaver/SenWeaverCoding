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

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    pending: SseEvent,
    ready: std::collections::VecDeque<SseEvent>,
    saw_data: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
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
                if self.pending.data.is_empty() {
                    self.pending.data.push_str(value);
                } else {
                    self.pending.data.push('\n');
                    self.pending.data.push_str(value);
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
