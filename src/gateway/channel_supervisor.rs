// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::Config;
use crate::config::LiveConfig;
use crate::gateway::AppState;
use crate::runtime::task_manager::TaskHandle;

pub struct ChannelSupervisor {
    handle: Mutex<Option<TaskHandle>>,
    live_config: LiveConfig,
}

impl ChannelSupervisor {
    fn new(live_config: LiveConfig) -> Self {
        Self {
            handle: Mutex::new(None),
            live_config,
        }
    }

    fn spawn_run(&self) {
        let mut guard = self.handle.lock();
        if let Some(existing) = guard.take() {
            existing.abort();
        }
        let cfg = self.live_config.load_ref();
        let join = crate::runtime::task_manager::spawn_supervised(
            "gateway.channels",
            async move {
                if let Err(err) = crate::channels::start_channels((*cfg).clone()).await {
                    tracing::warn!("channel runtime exited: {err:#}");
                }
            },
        );
        *guard = Some(join);
    }

    pub fn restart(&self) {
        self.spawn_run();
    }

    pub fn start_if_needed(&self, config: &Config) {
        if !has_supervised_channels(config) {
            return;
        }
        if self.handle.lock().is_some() {
            return;
        }
        self.spawn_run();
    }
}

static SUPERVISOR: std::sync::OnceLock<Arc<ChannelSupervisor>> = std::sync::OnceLock::new();

pub fn ensure_supervisor(live_config: &LiveConfig) -> Arc<ChannelSupervisor> {
    SUPERVISOR
        .get_or_init(|| Arc::new(ChannelSupervisor::new(live_config.clone())))
        .clone()
}

pub fn start_embedded_channels(config: &Config, live_config: &LiveConfig) {
    ensure_supervisor(live_config).start_if_needed(config);
}

pub async fn restart_channels(state: &AppState) -> Result<(), String> {
    let snapshot = state.config.lock().clone();
    state.push_live_config(snapshot);
    ensure_supervisor(&state.live_config).restart();
    Ok(())
}

fn has_supervised_channels(config: &Config) -> bool {
    config
        .channels_config
        .channels_except_webhook()
        .iter()
        .any(|(_, ok)| *ok)
}
