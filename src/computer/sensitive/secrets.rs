// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use once_cell::sync::Lazy;
use regex::Regex;

use super::detect::{resolve_overlaps, SensitiveCategory, SensitiveMatch, SensitiveSeverity};

const SECRET_RANK: u32 = 90;

struct SecretSpec {
    category: SensitiveCategory,
    label: &'static str,
    pattern: &'static Lazy<Regex>,
    value_group: usize,
}

static PRIVATE_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[A-Za-z0-9+/=\s]{0,4096}?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----)?")
        .expect("private key regex")
});
static GITHUB_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:gh[pousr]_[A-Za-z0-9]{20,255}|github_pat_[A-Za-z0-9_]{22,255})")
        .expect("github token regex")
});
static AWS_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}\b").expect("aws key regex")
});
static ANTHROPIC_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bsk-ant-[A-Za-z0-9_\-]{20,}").expect("anthropic key regex")
});
static OPENAI_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bsk-[A-Za-z0-9_\-]{20,}").expect("openai key regex")
});
static SLACK_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bxox[baprs]-[A-Za-z0-9\-]{10,}").expect("slack token regex")
});
static STRIPE_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b[sr]k_(?:live|test)_[A-Za-z0-9]{16,}").expect("stripe key regex")
});
static NPM_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bnpm_[A-Za-z0-9]{36}\b").expect("npm token regex")
});
static GOOGLE_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bAIza[0-9A-Za-z_\-]{35}\b").expect("google key regex")
});
static JWT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{5,}")
        .expect("jwt regex")
});
static CONNECTION_STRING_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\b(?:postgres|postgresql|mysql|mongodb(?:\+srv)?|redis|amqps?)://[^\s'"]{4,}:[^\s'"]{4,}@[^\s'"]{4,}"#)
        .expect("connection string regex")
});
static BEARER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bbearer\s+([A-Za-z0-9._+/=\-]{16,})").expect("bearer regex")
});
static PASSWORD_ASSIGN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\b(?:password|passwd|pwd)\s*[:=]\s*['"]?([^\s'"]{6,})"#)
        .expect("password assign regex")
});
static CREDENTIAL_ASSIGN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)\b(?:api[_\-]?key|apikey|secret[_\-]?key|client[_\-]?secret|access[_\-]?token|auth[_\-]?token|private[_\-]?token)\s*[:=]\s*['"]?([A-Za-z0-9._+/=\-]{12,})"#,
    )
    .expect("credential assign regex")
});

static SECRET_SPECS: [SecretSpec; 13] = [
    SecretSpec {
        category: SensitiveCategory::PrivateKey,
        label: "Private key",
        pattern: &PRIVATE_KEY_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "GitHub token",
        pattern: &GITHUB_TOKEN_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "AWS credential",
        pattern: &AWS_KEY_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "Anthropic API key",
        pattern: &ANTHROPIC_KEY_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "OpenAI API key",
        pattern: &OPENAI_KEY_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "Slack token",
        pattern: &SLACK_TOKEN_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "Stripe key",
        pattern: &STRIPE_KEY_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "npm token",
        pattern: &NPM_TOKEN_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "Google API key",
        pattern: &GOOGLE_KEY_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::Jwt,
        label: "JSON Web Token",
        pattern: &JWT_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "Database connection string",
        pattern: &CONNECTION_STRING_RE,
        value_group: 0,
    },
    SecretSpec {
        category: SensitiveCategory::ApiKey,
        label: "Bearer token",
        pattern: &BEARER_RE,
        value_group: 1,
    },
    SecretSpec {
        category: SensitiveCategory::Password,
        label: "Password",
        pattern: &PASSWORD_ASSIGN_RE,
        value_group: 1,
    },
];

pub fn scan_secrets(text: &str) -> Vec<SensitiveMatch> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for spec in &SECRET_SPECS {
        for caps in spec.pattern.captures_iter(text) {
            let Some(m) = caps.get(spec.value_group) else {
                continue;
            };
            if m.as_str().len() < 4 {
                continue;
            }
            out.push(SensitiveMatch {
                category: spec.category,
                label: spec.label,
                severity: SensitiveSeverity::High,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
                rank: SECRET_RANK,
            });
        }
    }
    for caps in CREDENTIAL_ASSIGN_RE.captures_iter(text) {
        if let Some(m) = caps.get(1) {
            out.push(SensitiveMatch {
                category: SensitiveCategory::ApiKey,
                label: "Credential",
                severity: SensitiveSeverity::High,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
                rank: 85,
            });
        }
    }
    resolve_overlaps(out)
}

pub fn scan_text(text: &str) -> Vec<SensitiveMatch> {
    let mut matches = scan_secrets(text);
    matches.extend(super::detect::scan_structured_pii(text));
    resolve_overlaps(matches)
}
