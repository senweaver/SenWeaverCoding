// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Product constants — mirrors claude-code-typescript-src`constants/product.ts`.

/// Product name.
pub const PRODUCT_NAME: &str = "SenWeaverCoding";

/// Product version (from Cargo.toml at compile time).
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Product description.
pub const PRODUCT_DESCRIPTION: &str = "AI Code Editor";

/// Default agent name displayed in prompts.
pub const DEFAULT_AGENT_NAME: &str = "Sen";

/// User-agent string for HTTP requests.
pub fn user_agent() -> String {
    format!("{PRODUCT_NAME}/{PRODUCT_VERSION}")
}

/// Config home directory name (under user home).
pub const CONFIG_HOME_DIR: &str = ".senweavercoding";
