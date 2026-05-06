// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::token_saver::CompactContext;

pub type FastPathFn =
    fn(&str, &str, &str, i32, &CompactContext) -> (String, String);

#[derive(Debug, Clone, Copy)]
pub enum HandlerKind {

    FastPath(FastPathFn),

    Toml(&'static str),

    Passthrough,
}

#[derive(Debug, Clone, Copy)]
pub struct RuleMatch {
    pub category: &'static str,
    pub handler: HandlerKind,
}

struct Rule {

    pattern: &'static str,
    category: &'static str,
    handler: HandlerKind,
}

static RULES: &[Rule] = &[

    Rule {
        pattern: r"^(?:git|yadm)\s+(?:-[Cc]\s+\S+\s+)*status\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::status),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+log\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::log),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+diff\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::diff),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+(?:add|stage)\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::add),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+commit\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::commit),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+push\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::push),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+pull\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::pull),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+fetch\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::generic_short_ack),
    },
    Rule {
        pattern: r"^(?:git|yadm)\s+(?:branch|checkout|switch|stash|tag)\b",
        category: "git",
        handler: HandlerKind::FastPath(super::fast_paths::git::generic_short_ack),
    },

    Rule {
        pattern: r"^cargo\s+(?:check|build)\b",
        category: "cargo",
        handler: HandlerKind::FastPath(super::fast_paths::cargo::build_or_check),
    },
    Rule {
        pattern: r"^cargo\s+clippy\b",
        category: "cargo",
        handler: HandlerKind::FastPath(super::fast_paths::cargo::build_or_check),
    },
    Rule {
        pattern: r"^cargo\s+test\b",
        category: "cargo",
        handler: HandlerKind::FastPath(super::fast_paths::cargo::test),
    },
    Rule {
        pattern: r"^cargo\s+(?:run|fmt|tree|update|fetch|metadata)\b",
        category: "cargo",
        handler: HandlerKind::FastPath(super::fast_paths::cargo::generic),
    },

    Rule {
        pattern: r"^(?:npm|pnpm|yarn|bun)\s+(?:install|add|remove|i|ci)\b",
        category: "npm",
        handler: HandlerKind::FastPath(super::fast_paths::npm::install),
    },
    Rule {
        pattern: r"^(?:npm|pnpm|yarn|bun)\s+(?:test|run\s+test)\b",
        category: "npm",
        handler: HandlerKind::FastPath(super::fast_paths::npm::test),
    },
    Rule {
        pattern: r"^(?:npm|pnpm|yarn|bun)\s+run\b",
        category: "npm",
        handler: HandlerKind::FastPath(super::fast_paths::npm::run),
    },
    Rule {
        pattern: r"^(?:tsc|eslint|biome)\b",
        category: "npm",
        handler: HandlerKind::FastPath(super::fast_paths::npm::lint_or_tsc),
    },

    Rule {
        pattern: r"^(?:pytest|python\s+-m\s+pytest)\b",
        category: "python",
        handler: HandlerKind::FastPath(super::fast_paths::python::pytest),
    },
    Rule {
        pattern: r"^ruff\b",
        category: "python",
        handler: HandlerKind::FastPath(super::fast_paths::python::ruff),
    },
    Rule {
        pattern: r"^pip\s+(?:list|outdated|install|show)\b",
        category: "python",
        handler: HandlerKind::FastPath(super::fast_paths::python::pip),
    },

    Rule {
        pattern: r"^ls\b",
        category: "system",
        handler: HandlerKind::FastPath(super::fast_paths::system::ls),
    },
    Rule {
        pattern: r"^find\b",
        category: "system",
        handler: HandlerKind::FastPath(super::fast_paths::system::find),
    },
    Rule {
        pattern: r"^(?:rg|grep|egrep|fgrep)\b",
        category: "system",
        handler: HandlerKind::FastPath(super::fast_paths::system::grep),
    },
    Rule {
        pattern: r"^(?:cat|head|tail)\b",
        category: "system",
        handler: HandlerKind::FastPath(super::fast_paths::system::cat),
    },
    Rule {
        pattern: r"^(?:du|df)\b",
        category: "system",
        handler: HandlerKind::Toml("du-df"),
    },
    Rule {
        pattern: r"^make\b",
        category: "system",
        handler: HandlerKind::Toml("make"),
    },
    Rule {
        pattern: r"^ping\b",
        category: "system",
        handler: HandlerKind::Toml("ping"),
    },
    Rule {
        pattern: r"^ps\b",
        category: "system",
        handler: HandlerKind::Toml("ps"),
    },
    Rule {
        pattern: r"^jq\b",
        category: "system",
        handler: HandlerKind::Toml("jq"),
    },

    Rule {
        pattern: r"^mvn\b",
        category: "build",
        handler: HandlerKind::Toml("mvn-build"),
    },
    Rule {
        pattern: r"^gradle\b",
        category: "build",
        handler: HandlerKind::Toml("gradle"),
    },
    Rule {
        pattern: r"^(?:terraform|tofu)\b",
        category: "build",
        handler: HandlerKind::Toml("terraform-plan"),
    },

    Rule {
        pattern: r"^(?:curl|wget|http)\b",
        category: "network",
        handler: HandlerKind::Passthrough,
    },
];

static COMPILED: Lazy<Vec<(Regex, &'static Rule)>> = Lazy::new(|| {
    RULES
        .iter()
        .filter_map(|r| match Regex::new(r.pattern) {
            Ok(re) => Some((re, r)),
            Err(e) => {
                tracing::error!(pattern = %r.pattern, error = %e, "bad dispatcher pattern; skipping");
                None
            }
        })
        .collect()
});

pub fn classify(command: &str) -> Option<RuleMatch> {
    let trimmed = command.trim_start();
    let stripped = crate::token_saver::mute::strip_no_compact_prefix(trimmed);
    for (re, rule) in COMPILED.iter() {
        if re.is_match(stripped) {
            return Some(RuleMatch {
                category: rule.category,
                handler: rule.handler,
            });
        }
    }
    None
}

pub fn list_rules() -> Vec<(&'static str, &'static str)> {
    RULES.iter().map(|r| (r.pattern, r.category)).collect()
}
