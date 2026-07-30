// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::time::Duration;

pub struct CtrlCAbortGuard {
    token: tokio_util::sync::CancellationToken,
}

impl CtrlCAbortGuard {
    pub fn install() -> Self {
        let token = tokio_util::sync::CancellationToken::new();
        let abort = token.clone();
        crate::runtime::spawn_supervised("cli.one_shot_ctrl_c", async move {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            abort.cancel();
            if tokio::signal::ctrl_c().await.is_ok() {
                std::process::exit(130);
            }
        });
        Self { token }
    }

    pub async fn aborted(&self) {
        self.token.cancelled().await;
    }

    pub async fn run_abortable<T>(
        &self,
        fut: impl std::future::Future<Output = T>,
    ) -> Option<T> {
        tokio::pin!(fut);
        let outcome = tokio::select! {
            r = &mut fut => Some(r),
            () = self.token.cancelled() => None,
        };
        if let Some(result) = outcome {
            return Some(result);
        }
        let killed = crate::tools::background::registry::kill_all();
        tracing::warn!(
            killed_shell_children = killed,
            "Ctrl+C received; aborting one-shot turn"
        );
        match tokio::time::timeout(Duration::from_secs(3), &mut fut).await {
            Ok(result) => Some(result),
            Err(_) => {
                crate::tools::background::registry::kill_all();
                None
            }
        }
    }
}
