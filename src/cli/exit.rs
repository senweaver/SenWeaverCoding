// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Centralized CLI exit helpers for subcommand handlers.
//!
//! Consolidates exit-with-message patterns used across CLI handlers.
//! `cli_error` writes to stderr and exits with code 1; `cli_ok` writes
//! to stdout and exits with code 0.

use std::io::Write;

/// Write an error message to stderr (if given) and exit with code 1.
pub fn cli_error(msg: Option<&str>) -> ! {
    if let Some(m) = msg {
        eprintln!("{m}");
    }
    std::process::exit(1)
}

/// Write a success message to stdout (if given) and exit with code 0.
pub fn cli_ok(msg: Option<&str>) -> ! {
    if let Some(m) = msg {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        let _ = writeln!(handle, "{m}");
        let _ = handle.flush();
    }
    std::process::exit(0)
}

/// Exit result type for handlers that may want to return errors
/// without immediately terminating.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_outcome_ok_has_message() {
        let outcome = ExitOutcome::ok("success");
        match outcome {
            ExitOutcome::Ok(Some(msg)) => assert_eq!(msg, "success"),
            _ => panic!("expected Ok with message"),
        }
    }

    #[test]
    fn exit_outcome_err_has_message() {
        let outcome = ExitOutcome::err("failure");
        match outcome {
            ExitOutcome::Error(msg) => assert_eq!(msg, "failure"),
            _ => panic!("expected Error"),
        }
    }
}
