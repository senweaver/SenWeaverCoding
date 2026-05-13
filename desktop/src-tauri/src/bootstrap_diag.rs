// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn install_tracing(log_dir: Option<&Path>) {
    if GUARD.get().is_some() {
        return;
    }

    let filter = EnvFilter::try_from_env("SEN_DESKTOP_LOG")
        .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_writer(std::io::stdout);

    let file_layer = log_dir.and_then(|dir| match prepare_log_dir(dir) {
        Ok(()) => {
            let appender = tracing_appender::rolling::daily(dir, "desktop-bootstrap.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let _ = GUARD.set(guard);
            let _ = LOG_PATH.set(dir.to_path_buf());
            Some(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_target(false)
                    .with_writer(writer),
            )
        }
        Err(err) => {
            eprintln!(
                "[sen-desktop] failed to prepare log dir {}: {err}",
                dir.display()
            );
            None
        }
    });

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init();
    if let Err(err) = result {
        eprintln!("[sen-desktop] tracing subscriber install failed: {err}");
    }
}

pub fn current_log_dir() -> Option<PathBuf> {
    LOG_PATH.get().cloned()
}

fn prepare_log_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    Ok(())
}
