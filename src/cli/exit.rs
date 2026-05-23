// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::io::Write;

pub fn cli_error(msg: Option<&str>) -> ! {
    if let Some(m) = msg {
        eprintln!("{m}");
    }
    std::process::exit(1)
}

pub fn cli_ok(msg: Option<&str>) -> ! {
    if let Some(m) = msg {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        let _ = writeln!(handle, "{m}");
        let _ = handle.flush();
    }
    std::process::exit(0)
}

#[derive(Debug)]
pub enum ExitOutcome {
    Ok(Option<String>),
    Error(String),
}

impl ExitOutcome {
    pub fn execute(self) -> ! {
        match self {
            Self::Ok(msg) => cli_ok(msg.as_deref()),
            Self::Error(msg) => cli_error(Some(&msg)),
        }
    }

    pub fn ok(msg: impl Into<String>) -> Self {
        Self::Ok(Some(msg.into()))
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self::Error(msg.into())
    }
}
