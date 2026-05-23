// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use tokio_util::sync::CancellationToken;

pub enum ToolRunOutcome<T> {

    Completed(T),

    Cancelled,
}

pub async fn run_or_cancel<F, T>(token: Option<&CancellationToken>, fut: F) -> ToolRunOutcome<T>
where
    F: std::future::Future<Output = T>,
{
    match token {
        Some(tok) => {
            tokio::select! {
                () = tok.cancelled() => ToolRunOutcome::Cancelled,
                v = fut => ToolRunOutcome::Completed(v),
            }
        }
        None => ToolRunOutcome::Completed(fut.await),
    }
}
