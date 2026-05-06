// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use globset::{Glob, GlobSet, GlobSetBuilder};
use once_cell::sync::Lazy;

const NO_COMPACT_PREFIX: &str = "NO_COMPACT ";

pub fn is_disabled_by_env() -> bool {
    matches!(
        std::env::var("SEN_TOKEN_SAVER_DISABLED").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

pub fn should_skip_command(command: &str) -> bool {
    if command.starts_with(NO_COMPACT_PREFIX) {
        return true;
    }
    static ALWAYS_SKIP: Lazy<GlobSet> = Lazy::new(|| {
        let mut b = GlobSetBuilder::new();
        for pat in [
            "vim*",
            "vi *",
            "nvim*",
            "less*",
            "more *",
            "top",
            "htop*",
            "btop*",
            "tmux*",
            "screen*",
            "fzf*",
            "watch *",
        ] {
            if let Ok(g) = Glob::new(pat) {
                b.add(g);
            }
        }
        b.build().unwrap_or_else(|_| GlobSet::empty())
    });
    let head = command.split_whitespace().next().unwrap_or("");
    ALWAYS_SKIP.is_match(head) || ALWAYS_SKIP.is_match(command)
}

pub fn strip_no_compact_prefix(command: &str) -> &str {
    command.strip_prefix(NO_COMPACT_PREFIX).unwrap_or(command)
}
