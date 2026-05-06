// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Cancellation-token helpers shared by the tool execution path.
//!
//! `loop_::execute_one_tool` supports an optional `CancellationToken`
//! that fires when the gateway cancels an in-flight turn (e.g. the
//! user pressed Ctrl-C in a TUI session, or a channel disconnected).
//! D2.1 pulls the `tokio::select!` boilerplate out so new call
//! sites get the same semantics for free.

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
