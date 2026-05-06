// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod cargo;
pub mod git;
pub mod npm;
pub mod python;
pub mod system;

use crate::token_saver::pipeline;

pub(crate) fn ansi_only(raw_stdout: &str, raw_stderr: &str) -> (String, String) {
    (
        pipeline::strip_ansi_only(raw_stdout),
        pipeline::strip_ansi_only(raw_stderr),
    )
}
