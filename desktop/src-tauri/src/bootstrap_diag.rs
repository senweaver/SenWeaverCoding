// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LAST_PANIC: OnceLock<Mutex<Option<String>>> = OnceLock::new();

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

    let resolved_dir = log_dir.and_then(resolve_writable_log_dir);

    let file_layer = resolved_dir.as_ref().map(|dir| {
        let appender = tracing_appender::rolling::daily(dir, "desktop-bootstrap.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        let _ = GUARD.set(guard);
        let _ = LOG_PATH.set(dir.clone());
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .with_writer(writer)
    });

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init();
    if let Err(err) = result {
        eprintln!("[sen-desktop] tracing subscriber install failed: {err}");
    }

    install_panic_hook();

    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "full");
    }
}

pub fn current_log_dir() -> Option<PathBuf> {
    LOG_PATH.get().cloned()
}

pub fn last_panic_message() -> Option<String> {
    LAST_PANIC.get().and_then(|m| m.lock().ok()?.clone())
}

fn resolve_writable_log_dir(dir: &Path) -> Option<PathBuf> {
    if let Err(err) = prepare_log_dir(dir) {
        eprintln!(
            "[sen-desktop] primary log dir {} unusable ({err}); trying %TEMP% fallback",
            dir.display()
        );
    } else if probe_log_dir_writable(dir) {
        return Some(dir.to_path_buf());
    }

    let mut fallback = std::env::temp_dir();
    fallback.push("SenAgentOS");
    fallback.push("logs");
    if let Err(err) = prepare_log_dir(&fallback) {
        eprintln!(
            "[sen-desktop] %TEMP% fallback log dir {} also unusable ({err}); \
             falling back to stdout-only",
            fallback.display()
        );
        return None;
    }
    if probe_log_dir_writable(&fallback) {
        eprintln!(
            "[sen-desktop] using %TEMP% fallback log dir {} \
             because primary dir was not writable",
            fallback.display()
        );
        Some(fallback)
    } else {
        None
    }
}

fn prepare_log_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    Ok(())
}

fn probe_log_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".sen-log-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn install_panic_hook() {
    let _ = LAST_PANIC.set(Mutex::new(None));
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        let payload = panic_payload_to_string(info.payload());
        let backtrace = std::backtrace::Backtrace::force_capture();
        let serialized = format!("panic at {location}: {payload}\nbacktrace:\n{backtrace}");
        if let Some(slot) = LAST_PANIC.get() {
            if let Ok(mut g) = slot.lock() {
                *g = Some(serialized.clone());
            }
        }
        write_crash_record(&serialized);
        tracing::error!("[sen-desktop] {serialized}");
        prev(info);
    }));
}

fn write_crash_record(serialized: &str) {
    use std::io::Write;

    let dir = LOG_PATH.get().cloned().unwrap_or_else(|| {
        let mut fallback = std::env::temp_dir();
        fallback.push("SenAgentOS");
        fallback.push("logs");
        fallback
    });
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let record = format!("===== crash @ unix {timestamp} (pid {}) =====\n{serialized}\n\n", std::process::id());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("desktop-crash.log"))
    {
        let _ = file.write_all(record.as_bytes());
        let _ = file.flush();
        let _ = file.sync_all();
    }
}

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}
