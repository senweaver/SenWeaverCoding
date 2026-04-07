// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Output styles module — mirrors claude-code's `outputStyles/` directory.
// Loads and manages custom output styles that modify agent response behaviour.

pub mod format;
pub mod loader;
pub mod types;

pub use format::{OutputStyle, format_tool_result};
pub use loader::load_output_styles;
pub use types::{OutputStyleDefinition, OutputStyleSource};
