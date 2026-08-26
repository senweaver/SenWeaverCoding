// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::LazyLock;

pub const DEFAULT_CONTEXT_WINDOW: u32 = 200_000;

pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16_384;

pub const EXTENDED_THINKING_MAX_OUTPUT: u32 = 65_536;

pub const MAX_TOOL_RESULTS_PER_TURN: u32 = 32;

pub const MAX_IMAGES_PER_TURN: u32 = 20;

pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;

pub const MAX_API_RETRIES: u32 = 3;

pub const API_TIMEOUT_MS: u64 = 600_000;

pub static MODEL_CONTEXT_WINDOWS: LazyLock<HashMap<&'static str, u32>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("claude-sonnet-4-20250514", 200_000);
    m.insert("claude-sonnet-4-5", 200_000);
    m.insert("claude-sonnet-4-5-20250929", 200_000);
    m.insert("claude-opus-4-20250514", 200_000);
    m.insert("claude-opus-4-1", 200_000);
    m.insert("claude-opus-4-5", 200_000);
    m.insert("claude-haiku-4-5", 200_000);
    m.insert("claude-3-5-sonnet-20241022", 200_000);
    m.insert("claude-3-5-haiku-20241022", 200_000);
    m.insert("claude-3-haiku-20240307", 200_000);
    m.insert("gpt-4o", 128_000);
    m.insert("gpt-4o-mini", 128_000);
    m.insert("gpt-4.1", 1_047_576);
    m.insert("gpt-4.1-mini", 1_047_576);
    m.insert("gpt-4.1-nano", 1_047_576);
    m.insert("gpt-5", 272_000);
    m.insert("gpt-5-mini", 272_000);
    m.insert("gpt-5-nano", 272_000);
    m.insert("gpt-5-codex", 272_000);
    m.insert("gpt-5.1", 272_000);
    m.insert("gpt-5.1-codex", 272_000);
    m.insert("gpt-5.2", 272_000);
    m.insert("gpt-5.2-codex", 272_000);
    m.insert("gpt-5.3", 272_000);
    m.insert("o1", 200_000);
    m.insert("o3", 200_000);
    m.insert("o4-mini", 200_000);
    m.insert("gemini-1.5-pro", 2_000_000);
    m.insert("gemini-1.5-flash", 1_000_000);
    m.insert("gemini-2.0-flash", 1_000_000);
    m.insert("gemini-2.5-pro", 1_000_000);
    m.insert("gemini-2.5-flash", 1_000_000);
    m.insert("gemini-3-pro", 1_000_000);
    m.insert("gemini-3-flash", 1_000_000);
    m.insert("gemini-3-pro-preview", 1_000_000);
    m.insert("deepseek-chat", 64_000);
    m.insert("deepseek-reasoner", 64_000);
    m.insert("deepseek-v3", 128_000);
    m.insert("deepseek-v3.1", 128_000);
    m.insert("deepseek-v4-pro", 1_000_000);
    m.insert("deepseek-v4-flash", 1_000_000);
    m.insert("moonshot-v1-8k", 8_192);
    m.insert("moonshot-v1-32k", 32_768);
    m.insert("moonshot-v1-128k", 128_000);
    m.insert("moonshot-v1-auto", 128_000);
    m.insert("moonshot-v1-8k-vision-preview", 8_192);
    m.insert("moonshot-v1-32k-vision-preview", 32_768);
    m.insert("moonshot-v1-128k-vision-preview", 128_000);
    m.insert("kimi-k2", 128_000);
    m.insert("kimi-k2-0711-preview", 128_000);
    m.insert("kimi-k2-0905-preview", 256_000);
    m.insert("kimi-k2-thinking", 256_000);
    m.insert("kimi-k2.5", 1_000_000);
    m.insert("kimi-k2.6", 1_000_000);
    m.insert("kimi-k2.7", 1_000_000);
    m.insert("moonshotai/kimi-k2.5", 1_000_000);
    m.insert("moonshotai/kimi-k2.6", 1_000_000);
    m.insert("kimi-for-coding", 256_000);
    m.insert("kimi-latest", 128_000);
    m.insert("kimi-thinking-preview", 128_000);
    m.insert("qwen-72b", 32_768);
    m
});

pub fn context_window_for_model(model: &str) -> u32 {
    if let Some(window) = MODEL_CONTEXT_WINDOWS.get(model).copied() {
        return window;
    }
    let id = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    if let Some(window) = MODEL_CONTEXT_WINDOWS.get(id.as_str()).copied() {
        return window;
    }
    if let Some(window) = infer_context_window_from_id(&id) {
        return window;
    }
    DEFAULT_CONTEXT_WINDOW
}

#[must_use]
pub fn thinking_context_cap_for_model(model: &str) -> Option<u32> {
    let id = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    if id.contains("deepseek-v4") {
        return Some(131_072);
    }
    None
}

fn infer_context_window_from_id(id: &str) -> Option<u32> {
    if id.starts_with("gpt-4.1") {
        return Some(1_047_576);
    }
    if id.starts_with("gpt-5") {
        return Some(272_000);
    }
    if id.contains("deepseek-v4") {
        return Some(1_000_000);
    }
    if id.contains("gemini-1.5-pro") {
        return Some(2_000_000);
    }
    if id.starts_with("gemini-1.5")
        || id.starts_with("gemini-2.0")
        || id.starts_with("gemini-2.5")
        || id.starts_with("gemini-3")
    {
        return Some(1_000_000);
    }
    if id.contains("moonshot-v1-8k") {
        return Some(8_192);
    }
    if id.contains("moonshot-v1-32k") {
        return Some(32_768);
    }
    if id.contains("moonshot-v1-128k") || id.contains("moonshot-v1-auto") {
        return Some(128_000);
    }
    if id.contains("kimi-k2.5")
        || id.contains("kimi-k2.6")
        || id.contains("kimi-k2.7")
        || id.contains("kimi-k2-1m")
    {
        return Some(1_000_000);
    }
    if id.contains("kimi-k2-thinking") {
        return Some(256_000);
    }
    if id.contains("kimi-k2-0905") {
        return Some(256_000);
    }
    if id.contains("kimi") {
        return Some(128_000);
    }
    if id.contains("-128k") {
        return Some(128_000);
    }
    if id.contains("-32k") {
        return Some(32_768);
    }
    if id.contains("-16k") {
        return Some(16_384);
    }
    if id.contains("-8k") {
        return Some(8_192);
    }
    if id.contains("-200k") {
        return Some(200_000);
    }
    if id.contains("-256k") {
        return Some(256_000);
    }
    if id.contains("-1m") {
        return Some(1_000_000);
    }
    None
}

pub fn max_output_for_model(model: &str) -> u32 {
    let id = model.rsplit('/').next().unwrap_or(model).to_ascii_lowercase();
    if id.contains("opus")
        || id.contains("sonnet-4")
        || id.contains("sonnet-4-5")
        || id.contains("haiku-4-5")
        || id.starts_with("gpt-5")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4")
    {
        EXTENDED_THINKING_MAX_OUTPUT
    } else {
        DEFAULT_MAX_OUTPUT_TOKENS
    }
}
