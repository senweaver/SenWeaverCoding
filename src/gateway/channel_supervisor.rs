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
        let live_config = self.live_config.clone();
        let join = crate::runtime::task_manager::spawn_supervised(
            "gateway.channels",
            async move {
                let initial_backoff_secs: u64 = 5;
                let max_backoff_secs: u64 = 60;
                let stable_run_secs: u64 = 300;
                let mut backoff_secs = initial_backoff_secs;
                loop {
                    let cfg = live_config.load_ref();
                    let started = std::time::Instant::now();
                    match crate::channels::start_channels((*cfg).clone()).await {
                        Ok(()) => {
                            tracing::warn!("channel runtime exited unexpectedly");
                        }
                        Err(err) => {
                            tracing::warn!("channel runtime exited: {err:#}");
                        }
                    }
                    if crate::gateway::lifecycle::is_shutdown_requested() {
                        tracing::info!(
                            "channel runtime supervisor stopping: gateway shutdown requested"
                        );
                        break;
                    }
                    if started.elapsed()
                        >= std::time::Duration::from_secs(stable_run_secs)
                    {
                        backoff_secs = initial_backoff_secs;
                    }
                    tracing::info!(
                        "channel runtime restarting in {backoff_secs}s"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                    backoff_secs = backoff_secs.saturating_mul(2).min(max_backoff_secs);
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
