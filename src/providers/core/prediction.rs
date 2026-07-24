// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

tokio::task_local! {
    static PREDICTED_OUTPUT: String;
}

pub async fn scope_predicted_output<F>(content: String, fut: F) -> F::Output
where
    F: std::future::Future,
{
    PREDICTED_OUTPUT.scope(content, fut).await
}

pub fn current_predicted_output() -> Option<String> {
    PREDICTED_OUTPUT.try_with(|p| p.clone()).ok()
}
