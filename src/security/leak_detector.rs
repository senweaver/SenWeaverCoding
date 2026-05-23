// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

const ENTROPY_TOKEN_MIN_LEN: usize = 24;

#[derive(Debug, Clone)]
pub enum LeakResult {

    Clean,

    Detected {

        patterns: Vec<String>,

        redacted: String,
    },
}

#[derive(Debug, Clone)]
pub struct LeakDetector {

    sensitivity: f64,
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl LeakDetector {

    pub fn new() -> Self {
        Self { sensitivity: 0.7 }
    }

    pub fn with_sensitivity(sensitivity: f64) -> Self {
        Self {
            sensitivity: sensitivity.clamp(0.0, 1.0),
        }
    }

    pub fn scan(&self, content: &str) -> LeakResult {
        let mut patterns = Vec::new();
        let mut redacted = content.to_string();

        self.check_api_keys(content, &mut patterns, &mut redacted);
        self.check_aws_credentials(content, &mut patterns, &mut redacted);
        self.check_generic_secrets(content, &mut patterns, &mut redacted);
        self.check_private_keys(content, &mut patterns, &mut redacted);
        self.check_jwt_tokens(content, &mut patterns, &mut redacted);
        self.check_database_urls(content, &mut patterns, &mut redacted);
        self.check_high_entropy_tokens(content, &mut patterns, &mut redacted);

        if patterns.is_empty() {
            LeakResult::Clean
        } else {
            LeakResult::Detected { patterns, redacted }
        }
    }

    fn check_api_keys(&self, content: &str, patterns: &mut Vec<String>, redacted: &mut String) {
        static API_KEY_PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
        let regexes = API_KEY_PATTERNS.get_or_init(|| {
            vec![

                (
                    Regex::new(r"sk_(live|test)_[a-zA-Z0-9]{24,}")
                        .expect("stripe secret key regex must compile"),
                    "Stripe secret key",
                ),
                (
                    Regex::new(r"pk_(live|test)_[a-zA-Z0-9]{24,}")
                        .expect("stripe publishable key regex must compile"),
                    "Stripe publishable key",
                ),

                (
                    Regex::new(r"sk-[a-zA-Z0-9]{20,}T3BlbkFJ[a-zA-Z0-9]{20,}")
                        .expect("OpenAI API key regex must compile"),
                    "OpenAI API key",
                ),
                (
                    Regex::new(r"sk-[a-zA-Z0-9]{48,}")
                        .expect("OpenAI-style API key regex must compile"),
                    "OpenAI-style API key",
                ),

                (
                    Regex::new(r"sk-ant-[a-zA-Z0-9-_]{32,}")
                        .expect("Anthropic API key regex must compile"),
                    "Anthropic API key",
                ),

                (
                    Regex::new(r"AIza[a-zA-Z0-9_-]{35}")
                        .expect("Google API key regex must compile"),
                    "Google API key",
                ),

                (
                    Regex::new(r"gh[pousr]_[a-zA-Z0-9]{36,}")
                        .expect("GitHub token regex must compile"),
                    "GitHub token",
                ),
                (
                    Regex::new(r"github_pat_[a-zA-Z0-9_]{22,}")
                        .expect("GitHub PAT regex must compile"),
                    "GitHub PAT",
                ),

                (
                    Regex::new(r#"api[_-]?key[=:]\s*['"]*[a-zA-Z0-9_-]{20,}"#)
                        .expect("generic API key regex must compile"),
                    "Generic API key",
                ),
            ]
        });

        for (regex, name) in regexes {
            if regex.is_match(content) {
                patterns.push(String::from(*name));
                *redacted = regex
                    .replace_all(redacted, "[REDACTED_API_KEY]")
                    .to_string();
            }
        }
    }

    fn check_aws_credentials(
        &self,
        content: &str,
        patterns: &mut Vec<String>,
        redacted: &mut String,
    ) {
        static AWS_PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
        let regexes = AWS_PATTERNS.get_or_init(|| {
            vec![
                (
                    Regex::new(r"AKIA[A-Z0-9]{16}")
                        .expect("AWS Access Key ID regex must compile"),
                    "AWS Access Key ID",
                ),
                (
                    Regex::new(
                        r#"aws[_-]?secret[_-]?access[_-]?key[=:]\s*['"]*[a-zA-Z0-9/+=]{40}"#,
                    )
                    .expect("AWS Secret Access Key regex must compile"),
                    "AWS Secret Access Key",
                ),
            ]
        });

        for (regex, name) in regexes {
            if regex.is_match(content) {
                patterns.push(String::from(*name));
                *redacted = regex
                    .replace_all(redacted, "[REDACTED_AWS_CREDENTIAL]")
                    .to_string();
            }
        }
    }

    fn check_generic_secrets(
        &self,
        content: &str,
        patterns: &mut Vec<String>,
        redacted: &mut String,
    ) {
        static SECRET_PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
        let regexes = SECRET_PATTERNS.get_or_init(|| {
            vec![
                (
                    Regex::new(r#"(?i)password[=:]\s*['"]*[^\s'"]{8,}"#)
                        .expect("password config regex must compile"),
                    "Password in config",
                ),
                (
                    Regex::new(r#"(?i)secret[=:]\s*['"]*[a-zA-Z0-9_-]{16,}"#)
                        .expect("secret value regex must compile"),
                    "Secret value",
                ),
                (
                    Regex::new(r#"(?i)token[=:]\s*['"]*[a-zA-Z0-9_.-]{20,}"#)
                        .expect("token value regex must compile"),
                    "Token value",
                ),
            ]
        });

        for (regex, name) in regexes {
            if regex.is_match(content) && self.sensitivity > 0.5 {
                patterns.push(String::from(*name));
                *redacted = regex.replace_all(redacted, "[REDACTED_SECRET]").to_string();
            }
        }
    }

    fn check_private_keys(&self, content: &str, patterns: &mut Vec<String>, redacted: &mut String) {

        let key_patterns = [
            (
                "-----BEGIN RSA PRIVATE KEY-----",
                "-----END RSA PRIVATE KEY-----",
                "RSA private key",
            ),
            (
                "-----BEGIN EC PRIVATE KEY-----",
                "-----END EC PRIVATE KEY-----",
                "EC private key",
            ),
            (
                "-----BEGIN PRIVATE KEY-----",
                "-----END PRIVATE KEY-----",
                "Private key",
            ),
            (
                "-----BEGIN OPENSSH PRIVATE KEY-----",
                "-----END OPENSSH PRIVATE KEY-----",
                "OpenSSH private key",
            ),
        ];

        for (begin, end, name) in key_patterns {
            if content.contains(begin) && content.contains(end) {
                patterns.push(name.to_string());

                if let Some(start_idx) = content.find(begin) {
                    if let Some(end_idx) = content.find(end) {
                        let key_block = &content[start_idx..end_idx + end.len()];
                        *redacted = redacted.replace(key_block, "[REDACTED_PRIVATE_KEY]");
                    }
                }
            }
        }
    }

    fn check_jwt_tokens(&self, content: &str, patterns: &mut Vec<String>, redacted: &mut String) {
        static JWT_PATTERN: OnceLock<Regex> = OnceLock::new();
        let regex = JWT_PATTERN.get_or_init(|| {

            Regex::new(r"eyJ[a-zA-Z0-9_-]*\.eyJ[a-zA-Z0-9_-]*\.[a-zA-Z0-9_-]*")
                .expect("JWT token regex must compile")
        });

        if regex.is_match(content) {
            patterns.push("JWT token".to_string());
            *redacted = regex.replace_all(redacted, "[REDACTED_JWT]").to_string();
        }
    }

    fn check_database_urls(
        &self,
        content: &str,
        patterns: &mut Vec<String>,
        redacted: &mut String,
    ) {
        static DB_PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
        let regexes = DB_PATTERNS.get_or_init(|| {
            vec![
                (
                    Regex::new(r"postgres(ql)?://[^:]+:[^@]+@[^\s]+")
                        .expect("postgres URL regex must compile"),
                    "PostgreSQL connection URL",
                ),
                (
                    Regex::new(r"mysql://[^:]+:[^@]+@[^\s]+")
                        .expect("mysql URL regex must compile"),
                    "MySQL connection URL",
                ),
                (
                    Regex::new(r"mongodb(\+srv)?://[^:]+:[^@]+@[^\s]+")
                        .expect("mongodb URL regex must compile"),
                    "MongoDB connection URL",
                ),
                (
                    Regex::new(r"redis://[^:]+:[^@]+@[^\s]+")
                        .expect("redis URL regex must compile"),
                    "Redis connection URL",
                ),
            ]
        });

        for (regex, name) in regexes {
            if regex.is_match(content) {
                patterns.push(String::from(*name));
                *redacted = regex
                    .replace_all(redacted, "[REDACTED_DATABASE_URL]")
                    .to_string();
            }
        }
    }

    fn check_high_entropy_tokens(
        &self,
        content: &str,
        patterns: &mut Vec<String>,
        redacted: &mut String,
    ) {

        let entropy_threshold = 3.5 + self.sensitivity * 1.25;

        static URL_PATTERN: OnceLock<Regex> = OnceLock::new();
        let url_re = URL_PATTERN
            .get_or_init(|| Regex::new(r"https?://\S+").expect("URL regex must compile"));
        static MEDIA_MARKER_PATTERN: OnceLock<Regex> = OnceLock::new();
        let media_re = MEDIA_MARKER_PATTERN.get_or_init(|| {
            Regex::new(r"\[(IMAGE|VIDEO|VOICE|AUDIO|DOCUMENT|FILE):[^\]]*\]")
                .expect("media marker regex must compile")
        });
        let content_stripped = url_re.replace_all(content, "");
        let content_without_urls = media_re.replace_all(&content_stripped, "");

        let tokens = extract_candidate_tokens(&content_without_urls);

        for token in tokens {
            if token.len() >= ENTROPY_TOKEN_MIN_LEN {
                let entropy = shannon_entropy(token);
                if entropy >= entropy_threshold && has_mixed_alpha_digit(token) {
                    patterns.push("High-entropy token".to_string());
                    *redacted = redacted.replace(token, "[REDACTED_HIGH_ENTROPY_TOKEN]");
                }
            }
        }
    }
}

fn extract_candidate_tokens(content: &str) -> Vec<&str> {
    content
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '+' && c != '/')
        .filter(|s| !s.is_empty())
        .collect()
}

fn shannon_entropy(s: &str) -> f64 {
    let len = s.len() as f64;
    if len == 0.0 {
        return 0.0;
    }
    let mut freq: HashMap<u8, usize> = HashMap::new();
    for &b in s.as_bytes() {
        *freq.entry(b).or_insert(0) += 1;
    }
    freq.values().fold(0.0, |acc, &count| {
        let p = count as f64 / len;
        acc - p * p.log2()
    })
}

fn has_mixed_alpha_digit(s: &str) -> bool {
    let has_alpha = s.bytes().any(|b| b.is_ascii_alphabetic());
    let has_digit = s.bytes().any(|b| b.is_ascii_digit());
    has_alpha && has_digit
}
