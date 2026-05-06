// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Product constants — mirrors claude-code-typescript-src`constants/product.ts`.

pub const PRODUCT_NAME: &str = "SenWeaverCoding";

pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const PRODUCT_DESCRIPTION: &str = "AI Code Editor";

pub const DEFAULT_AGENT_NAME: &str = "Sen";

pub fn user_agent() -> String {
    format!("{PRODUCT_NAME}/{PRODUCT_VERSION}")
}

pub const CONFIG_HOME_DIR: &str = ".senweavercoding";
