// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod classifier;

pub use classifier::{ErrorClass, classify_tool_error};

pub struct Recovery;

impl Recovery {

    #[inline]
    pub fn classify_error(err: &anyhow::Error) -> ErrorClass {
        classify_tool_error(err)
    }
}
