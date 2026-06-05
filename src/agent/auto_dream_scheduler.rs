// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::time::Duration;

use crate::services::auto_dream::DreamTask;
use crate::services::ServiceContainer;

const TICK_SECS: u64 = 15;
const MIN_SYSTEM_IDLE_MS: u64 = 30_000;

pub async fn run() {
    let mut was_active = false;
    loop {
        tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;

        let Some(svc) = crate::services::try_get_services() else {
            was_active = false;
            continue;
        };

        let currently_active = crate::agent::activity::active_turns() > 0;

        if !svc.auto_dream.is_enabled().await {
            was_active = currently_active;
            continue;
        }

        if was_active && !currently_active {
            for task in svc.auto_dream.session_end_tasks().await {
                launch(svc, task);
            }
        }
        was_active = currently_active;

        if crate::agent::activity::is_idle(MIN_SYSTEM_IDLE_MS) {
            for task in svc.auto_dream.pending_tasks(now_ms(), true).await {
                launch(svc, task);
            }
        }
    }
}

fn launch(svc: &'static ServiceContainer, task: DreamTask) {
    crate::runtime::task_manager::spawn_supervised("auto_dream.task", async move {
        if !svc.auto_dream.try_begin(&task.id).await {
            return;
        }

        let config = (*svc.config()).clone();
        let temperature = config.default_temperature;
        let allowed = if task.allowed_tools.is_empty() {
            None
        } else {
            Some(task.allowed_tools.clone())
        };
        let duration = Duration::from_millis(task.max_duration_ms.max(1_000));

        let fut = crate::agent::run(
            config,
            Some(task.prompt.clone()),
            None,
            None,
            temperature,
            Vec::new(),
            false,
            None,
            allowed,
            None,
        );

        match tokio::time::timeout(duration, fut).await {
            Ok(Ok(_)) => {
                tracing::info!(target: "auto_dream", task = %task.id, "auto_dream task completed");
            }
            Ok(Err(err)) => {
                tracing::warn!(target: "auto_dream", task = %task.id, error = %err, "auto_dream task failed");
            }
            Err(_) => {
                tracing::warn!(target: "auto_dream", task = %task.id, "auto_dream task timed out");
            }
        }

        svc.auto_dream.mark_done(&task.id).await;
    });
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
