// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use anyhow::{Result, bail};
use std::io::{BufRead, Read, Write};

pub fn read_line_lossy<R: BufRead>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut raw = Vec::new();
    let n = reader.read_until(b'\n', &mut raw)?;
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&raw).into_owned()))
}

pub fn read_stdin_line_lossy() -> std::io::Result<Option<String>> {
    read_line_lossy(&mut std::io::stdin().lock())
}

// Piped stdin on Windows consoles frequently arrives as GBK (or another ANSI
// code page) rather than UTF-8; decode with the same best-effort detection the
// file tools use instead of failing the whole read on the first invalid byte.
pub fn read_stdin_to_string_best_effort() -> std::io::Result<String> {
    let mut raw = Vec::new();
    std::io::stdin().lock().read_to_end(&mut raw)?;
    let (text, _label) = crate::tools::file::encoding::decode_best_effort(&raw);
    Ok(text)
}

#[derive(Debug, Clone, Default)]
pub struct Input {
    prompt: String,
    default: Option<String>,
    allow_empty: bool,
}

impl Input {
    #[must_use]
    pub fn new() -> Self {
        Self {
            prompt: String::new(),
            default: None,
            allow_empty: false,
        }
    }

    #[must_use]
    pub fn with_prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.prompt = prompt.into();
        self
    }

    #[must_use]
    pub fn allow_empty(mut self, val: bool) -> Self {
        self.allow_empty = val;
        self
    }

    #[must_use]
    pub fn default<S: Into<String>>(mut self, value: S) -> Self {
        self.default = Some(value.into());
        self
    }

    pub fn interact_text(self) -> Result<String> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        self.interact_text_with_io(stdin.lock(), stdout.lock())
    }

    fn interact_text_with_io<R: BufRead, W: Write>(
        self,
        mut reader: R,
        mut writer: W,
    ) -> Result<String> {
        loop {
            write!(writer, "{}", self.render_prompt())?;
            writer.flush()?;

            let Some(line) = read_line_lossy(&mut reader)? else {
                bail!("No input received from stdin");
            };

            let trimmed = trim_trailing_line_ending(&line);
            if trimmed.is_empty() {
                if let Some(default) = &self.default {
                    return Ok(default.clone());
                }
                if self.allow_empty {
                    return Ok(String::new());
                }
                writeln!(writer, "Input cannot be empty.")?;
                continue;
            }

            return Ok(trimmed.to_string());
        }
    }

    fn render_prompt(&self) -> String {
        match &self.default {
            Some(default) => format!("{} [{}]: ", self.prompt, default),
            None => format!("{}: ", self.prompt),
        }
    }
}

fn trim_trailing_line_ending(input: &str) -> &str {
    input.trim_end_matches(['\n', '\r'])
}
