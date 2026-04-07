// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Vim mode module — mirrors claude-code's `vim/` directory.
// Provides vim-style editing capabilities for the terminal input.

pub mod motions;
pub mod operators;
pub mod text_objects;
pub mod transitions;
pub mod types;

pub use types::{VimAction, VimMode, VimState};
