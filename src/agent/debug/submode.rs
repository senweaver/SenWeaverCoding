// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugSubMode {
    Auto,
    CodeReview,
    SecurityReview,
    E2e,
    Performance,
}

impl DebugSubMode {
    pub fn all() -> &'static [DebugSubMode] {
        &[
            Self::Auto,
            Self::CodeReview,
            Self::SecurityReview,
            Self::E2e,
            Self::Performance,
        ]
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(Self::Auto),
            "code-review" | "code_review" | "codereview" | "review" => Some(Self::CodeReview),
            "security-review" | "security_review" | "security" | "audit" => {
                Some(Self::SecurityReview)
            }
            "e2e" | "e2e-testing" | "e2e_testing" | "test" | "testing" => Some(Self::E2e),
            "performance" | "perf" | "load" | "load-test" | "stress" => Some(Self::Performance),
            _ => None,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::CodeReview => "code-review",
            Self::SecurityReview => "security-review",
            Self::E2e => "e2e",
            Self::Performance => "performance",
        }
    }

    pub fn label_en(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::CodeReview => "Code Review",
            Self::SecurityReview => "Security Review",
            Self::E2e => "E2E Testing",
            Self::Performance => "Performance & Load",
        }
    }

    pub fn label_zh(&self) -> &'static str {
        match self {
            Self::Auto => "自动",
            Self::CodeReview => "代码审查",
            Self::SecurityReview => "安全审查",
            Self::E2e => "端到端测试",
            Self::Performance => "性能与负载",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Auto => "🧭",
            Self::CodeReview => "🔍",
            Self::SecurityReview => "🛡",
            Self::E2e => "🧪",
            Self::Performance => "⚡",
        }
    }

    pub fn may_write(&self) -> bool {
        matches!(self, Self::E2e | Self::Performance)
    }
}
