// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::Serialize;
use std::path::PathBuf;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PythonEnvEvent {
    Creating {
        workspace: PathBuf,
        tool: String,
    },
    Progress {
        workspace: PathBuf,
        message: String,
    },
    Ready {
        workspace: PathBuf,
        interpreter: PathBuf,
        version: Option<String>,
        #[serde(rename = "fallbackUsed")]
        fallback_used: bool,
    },
    Failed {
        workspace: PathBuf,
        error: String,
    },
    InstallStart {
        workspace: PathBuf,
        file: PathBuf,
    },
    InstallProgress {
        workspace: PathBuf,
        line: String,
    },
    InstallDone {
        workspace: PathBuf,
        success: bool,
        message: Option<String>,
    },
    PackagesCounted {
        workspace: PathBuf,
        count: u32,
    },
    Purged {
        workspace: PathBuf,
    },
}

static GLOBAL_BUS: std::sync::OnceLock<broadcast::Sender<PythonEnvEvent>> =
    std::sync::OnceLock::new();

fn bus() -> &'static broadcast::Sender<PythonEnvEvent> {
    GLOBAL_BUS.get_or_init(|| {
        let (tx, _rx) = broadcast::channel(128);
        tx
    })
}

pub fn publish(event: PythonEnvEvent) {
    let _ = bus().send(event);
}

pub fn subscribe_events() -> broadcast::Receiver<PythonEnvEvent> {
    bus().subscribe()
}
