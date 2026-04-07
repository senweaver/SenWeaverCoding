// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// API limits — mirrors claude-code-typescript-src`constants/apiLimits.ts`.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Default context window size (tokens).
pub const DEFAULT_CONTEXT_WINDOW: u32 = 200_000;

/// Default maximum output tokens.
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16_384;

/// Maximum output tokens for extended-thinking models.
pub const EXTENDED_THINKING_MAX_OUTPUT: u32 = 65_536;

/// Maximum number of tool results per turn.
pub const MAX_TOOL_RESULTS_PER_TURN: u32 = 32;

/// Maximum number of images per turn.
pub const MAX_IMAGES_PER_TURN: u32 = 20;

/// Maximum image size in bytes (25 MB).
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;

/// Maximum number of retries for API calls.
pub const MAX_API_RETRIES: u32 = 3;

/// Default API timeout in milliseconds.
pub const API_TIMEOUT_MS: u64 = 600_000; // 10 minutes

/// Context window sizes per model.
pub static MODEL_CONTEXT_WINDOWS: LazyLock<HashMap<&'static str, u32>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("claude-sonnet-4-20250514", 200_000);
    m.insert("claude-opus-4-20250514", 200_000);
    m.insert("claude-3-5-sonnet-20241022", 200_000);
    m.insert("claude-3-5-haiku-20241022", 200_000);
    m.insert("claude-3-haiku-20240307", 200_000);
    m.insert("gpt-4o", 128_000);
    m.insert("gpt-4o-mini", 128_000);
    m.insert("deepseek-chat", 64_000);
    m.insert("deepseek-reasoner", 64_000);
    m
});

/// Get context window for a model (falls back to default).
pub fn context_window_for_model(model: &str) -> u32 {
    MODEL_CONTEXT_WINDOWS
        .get(model)
        .copied()
        .unwrap_or(DEFAULT_CONTEXT_WINDOW)
}

/// Get max output tokens for a model.
pub fn max_output_for_model(model: &str) -> u32 {
    if model.contains("opus") || model.contains("sonnet-4") {
        EXTENDED_THINKING_MAX_OUTPUT
    } else {
        DEFAULT_MAX_OUTPUT_TOKENS
    }
}
