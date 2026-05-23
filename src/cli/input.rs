// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use anyhow::{Result, bail};
use std::io::{BufRead, Write};

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

            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                bail!("No input received from stdin");
            }

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
