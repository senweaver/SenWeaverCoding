// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Constants module — mirrors claude-code's `constants/` directory.
// Centralized constants for API limits, tool limits, prompts, product info,
// system settings, output styles, and XML tags.

pub mod api_limits;
pub mod files;
pub mod output_styles;
pub mod product;
pub mod prompts;
pub mod system;
pub mod tool_limits;
pub mod xml;

pub use api_limits::{DEFAULT_CONTEXT_WINDOW, DEFAULT_MAX_OUTPUT_TOKENS, MODEL_CONTEXT_WINDOWS};
pub use product::{PRODUCT_NAME, PRODUCT_VERSION};
pub use tool_limits::{MAX_TOOL_OUTPUT_CHARS, TOOL_TIMEOUT_MS};
