// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Out-bound PII sanitizer for the Debug coding mode.
//
// This module provides a single, deterministic redaction layer that sits
// at the LLM boundary. It is intentionally protocol-agnostic: it operates
// on plain strings and on the JSON envelopes that this codebase uses for
// assistant tool_calls and tool result messages, so the same sanitizer
// works for both the OpenAI Chat Completions wire shape (tool_calls /
// tool) and the Anthropic Messages wire shape (tool_use / tool_result)
// after they have been normalized into our unified ChatMessage list.
//
// Sanitization is stable: repeated runs over already-redacted text are
// no-ops, and every match is replaced by a category-named placeholder so
// the downstream model can still reason about the *kind* of value that
// was elided.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiKind {
    IdCard,
    Phone,
    Email,
    BankCard,
    Jwt,
    ApiKey,
    Bearer,
    AuthHeader,
    UrlPassword,
    KvSecret,
    PrivateKey,
    Ipv4,
    MacAddress,
}

impl PiiKind {
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::IdCard => "[REDACTED:ID_CARD]",
            Self::Phone => "[REDACTED:PHONE]",
            Self::Email => "[REDACTED:EMAIL]",
            Self::BankCard => "[REDACTED:BANK_CARD]",
            Self::Jwt => "[REDACTED:JWT]",
            Self::ApiKey => "[REDACTED:API_KEY]",
            Self::Bearer => "[REDACTED:BEARER]",
            Self::AuthHeader => "[REDACTED:AUTH_HEADER]",
            Self::UrlPassword => "[REDACTED:URL_PASSWORD]",
            Self::KvSecret => "[REDACTED:SECRET]",
            Self::PrivateKey => "[REDACTED:PRIVATE_KEY]",
            Self::Ipv4 => "[REDACTED:IPV4]",
            Self::MacAddress => "[REDACTED:MAC]",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::IdCard => "id_card",
            Self::Phone => "phone",
            Self::Email => "email",
            Self::BankCard => "bank_card",
            Self::Jwt => "jwt",
            Self::ApiKey => "api_key",
            Self::Bearer => "bearer",
            Self::AuthHeader => "auth_header",
            Self::UrlPassword => "url_password",
            Self::KvSecret => "kv_secret",
            Self::PrivateKey => "private_key",
            Self::Ipv4 => "ipv4",
            Self::MacAddress => "mac",
        }
    }

    pub fn all() -> &'static [PiiKind] {
        &[
            Self::IdCard,
            Self::Phone,
            Self::Email,
            Self::BankCard,
            Self::Jwt,
            Self::ApiKey,
            Self::Bearer,
            Self::AuthHeader,
            Self::UrlPassword,
            Self::KvSecret,
            Self::PrivateKey,
            Self::Ipv4,
            Self::MacAddress,
        ]
    }

    pub fn from_label(label: &str) -> Option<Self> {
        let alias = match label.to_ascii_lowercase().as_str() {
            "authorization_header" | "authheader" => Some(Self::AuthHeader),
            "id-card" | "idcard" => Some(Self::IdCard),
            "bank-card" | "bankcard" => Some(Self::BankCard),
            "api-key" | "apikey" => Some(Self::ApiKey),
            "url-password" | "urlpassword" => Some(Self::UrlPassword),
            "kv-secret" | "kvsecret" | "secret" => Some(Self::KvSecret),
            "private-key" | "privatekey" => Some(Self::PrivateKey),
            "ipv4_address" | "ip" => Some(Self::Ipv4),
            "mac_address" | "mac-address" => Some(Self::MacAddress),
            _ => None,
        };
        if alias.is_some() {
            return alias;
        }
        Self::all()
            .iter()
            .copied()
            .find(|kind| kind.label().eq_ignore_ascii_case(label))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SanitizationReport {
    pub counts: HashMap<PiiKind, u32>,
}

impl SanitizationReport {
    pub fn is_empty(&self) -> bool {
        self.counts.values().all(|c| *c == 0)
    }

    pub fn total(&self) -> u32 {
        self.counts.values().sum()
    }

    pub fn merge(&mut self, other: &SanitizationReport) {
        for (kind, count) in &other.counts {
            *self.counts.entry(*kind).or_default() += *count;
        }
    }

    pub fn bump(&mut self, kind: PiiKind, n: u32) {
        if n == 0 {
            return;
        }
        *self.counts.entry(kind).or_default() += n;
    }

    pub fn to_label_map(&self) -> HashMap<String, u32> {
        self.counts
            .iter()
            .map(|(k, v)| (k.label().to_string(), *v))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct PiiSanitizerConfig {
    pub enabled: bool,
    pub disabled_kinds: Vec<PiiKind>,
}

impl Default for PiiSanitizerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            disabled_kinds: vec![PiiKind::Ipv4, PiiKind::MacAddress],
        }
    }
}

impl PiiSanitizerConfig {
    pub fn from_settings(value: &serde_json::Value) -> Self {
        let mut cfg = Self::default();
        if let Some(enabled) = value.get("enabled").and_then(|v| v.as_bool()) {
            cfg.enabled = enabled;
        }
        if let Some(disabled) = value.get("disabledKinds").and_then(|v| v.as_array()) {
            let mut kinds = Vec::new();
            for entry in disabled {
                if let Some(label) = entry.as_str() {
                    if let Some(kind) = PiiKind::from_label(label) {
                        kinds.push(kind);
                    }
                }
            }
            cfg.disabled_kinds = kinds;
        }
        cfg
    }

    pub fn is_kind_enabled(&self, kind: PiiKind) -> bool {
        self.enabled && !self.disabled_kinds.contains(&kind)
    }
}

pub struct PiiSanitizer {
    config: RwLock<PiiSanitizerConfig>,
    rules: Vec<SanitizerRule>,
}

struct SanitizerRule {
    kind: PiiKind,
    regex: Regex,

    validator: Option<fn(&str) -> bool>,

    replacement: ReplacementMode,
}

enum ReplacementMode {
    Plain,
    KvKeepKey,
    UrlPassword,
}

impl PiiSanitizer {
    pub fn new(config: PiiSanitizerConfig) -> Self {
        Self {
            config: RwLock::new(config),
            rules: build_rules(),
        }
    }

    pub fn update_config(&self, config: PiiSanitizerConfig) {
        *self.config.write() = config;
    }

    pub fn config_snapshot(&self) -> PiiSanitizerConfig {
        self.config.read().clone()
    }

    pub fn enabled(&self) -> bool {
        self.config.read().enabled
    }

    pub fn sanitize(&self, input: &str) -> (String, SanitizationReport) {
        let mut report = SanitizationReport::default();
        if input.is_empty() {
            return (String::new(), report);
        }

        let cfg = self.config.read().clone();
        if !cfg.enabled {
            return (input.to_string(), report);
        }

        let mut current = input.to_string();
        for rule in &self.rules {
            if !cfg.is_kind_enabled(rule.kind) {
                continue;
            }
            current = apply_rule(&current, rule, &mut report);
        }
        (current, report)
    }

    pub fn sanitize_json(&self, value: &serde_json::Value) -> (serde_json::Value, SanitizationReport) {
        let mut report = SanitizationReport::default();
        let new_value = self.sanitize_json_inner(value, &mut report);
        (new_value, report)
    }

    fn sanitize_json_inner(
        &self,
        value: &serde_json::Value,
        report: &mut SanitizationReport,
    ) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => {
                let (cleaned, sub_report) = self.sanitize(s);
                report.merge(&sub_report);
                serde_json::Value::String(cleaned)
            }
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter()
                    .map(|v| self.sanitize_json_inner(v, report))
                    .collect(),
            ),
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    out.insert(k.clone(), self.sanitize_json_inner(v, report));
                }
                serde_json::Value::Object(out)
            }
            other => other.clone(),
        }
    }
}

fn apply_rule(input: &str, rule: &SanitizerRule, report: &mut SanitizationReport) -> String {
    let mut count: u32 = 0;
    let placeholder = rule.kind.placeholder();
    let cleaned = rule.regex.replace_all(input, |caps: &regex::Captures<'_>| {
        let full = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        if full.is_empty() {
            return String::new();
        }

        if full.contains("[REDACTED:") {
            return full.to_string();
        }

        if let Some(validator) = rule.validator {
            let target = caps.get(1).map(|m| m.as_str()).unwrap_or(full);
            if !validator(target) {
                return full.to_string();
            }
        }

        count += 1;
        match rule.replacement {
            ReplacementMode::Plain => placeholder.to_string(),
            ReplacementMode::KvKeepKey => {
                let key = caps.name("key").map(|m| m.as_str()).unwrap_or("secret");
                let assign = caps.name("assign").map(|m| m.as_str()).unwrap_or("=");
                let opener = caps.name("open").map(|m| m.as_str()).unwrap_or("");
                let closer = caps.name("close").map(|m| m.as_str()).unwrap_or("");
                format!("{key}{assign}{opener}{placeholder}{closer}")
            }
            ReplacementMode::UrlPassword => {
                let user = caps.name("user").map(|m| m.as_str()).unwrap_or("");
                format!("{user}:{placeholder}@")
            }
        }
    });

    if count > 0 {
        report.bump(rule.kind, count);
    }
    cleaned.into_owned()
}

fn build_rules() -> Vec<SanitizerRule> {
    let mut rules: Vec<SanitizerRule> = Vec::new();

    rules.push(SanitizerRule {
        kind: PiiKind::PrivateKey,
        regex: Regex::new(
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----[\s\S]*?-----END (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----",
        )
        .expect("private key regex"),
        validator: None,
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::Jwt,
        regex: Regex::new(r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\b")
            .expect("jwt regex"),
        validator: None,
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::AuthHeader,
        regex: Regex::new(
            r"(?im)^(?P<key>Authorization|Proxy-Authorization|X-Api-Key|X-Auth-Token)(?P<assign>\s*:\s*)(?P<open>)(?P<value>[^\r\n]+)(?P<close>)$",
        )
        .expect("auth header regex"),
        validator: None,
        replacement: ReplacementMode::KvKeepKey,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::Bearer,
        regex: Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9\-_\.=:+/]{16,}").expect("bearer regex"),
        validator: None,
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::ApiKey,
        regex: Regex::new(
            r"\b(?:sk-[A-Za-z0-9_\-]{16,}|sk_(?:live|test)_[A-Za-z0-9]{16,}|rk_(?:live|test)_[A-Za-z0-9]{16,}|pk_(?:live|test)_[A-Za-z0-9]{16,}|AKIA[0-9A-Z]{16}|ASIA[0-9A-Z]{16}|AIza[0-9A-Za-z\-_]{20,}|ya29\.[A-Za-z0-9\-_]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_\-]{20,}|xox[abprs]-[A-Za-z0-9\-]{10,}|EAACEdEose0cBA[A-Za-z0-9]{20,})\b",
        )
        .expect("api key regex"),
        validator: None,
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::KvSecret,
        regex: Regex::new(
            r#"(?i)(?P<key>password|passwd|pwd|secret|api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|auth[_-]?token|session[_-]?token|private[_-]?key)(?P<assign>\s*[:=]\s*)(?:(?P<open>")(?P<value>[^"\r\n]{4,256})(?P<close>")|(?P<open2>')(?P<value2>[^'\r\n]{4,256})(?P<close2>')|(?P<value3>[A-Za-z0-9!#$%&*+\-./:?@^_~\\|]{4,256}))"#,
        )
        .expect("kv secret regex"),
        validator: None,
        replacement: ReplacementMode::KvKeepKey,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::UrlPassword,
        regex: Regex::new(
            r"(?i)(?P<scheme>[a-z][a-z0-9+.\-]*)://(?P<user>[A-Za-z0-9._~%\-]+):(?P<pwd>[^\s@/]+)@",
        )
        .expect("url password regex"),
        validator: None,
        replacement: ReplacementMode::UrlPassword,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::IdCard,
        regex: Regex::new(r"(?:^|[^0-9A-Za-z])((?:[1-9]\d{16}[\dXx])|(?:[1-9]\d{14}))(?:$|[^0-9A-Za-z])")
            .expect("id card regex"),
        validator: Some(validate_china_id_card),
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::BankCard,
        regex: Regex::new(r"(?:^|[^0-9])((?:\d[ \-]?){12,18}\d)(?:$|[^0-9])")
            .expect("bank card regex"),
        validator: Some(validate_luhn),
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::Phone,
        regex: Regex::new(r"(?:^|[^0-9+])(\+?(?:86)?1[3-9]\d{9})(?:$|[^0-9])")
            .expect("phone regex"),
        validator: None,
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::Email,
        regex: Regex::new(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b")
            .expect("email regex"),
        validator: None,
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::Ipv4,
        regex: Regex::new(
            r"\b(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\b",
        )
        .expect("ipv4 regex"),
        validator: Some(validate_non_loopback_ipv4),
        replacement: ReplacementMode::Plain,
    });

    rules.push(SanitizerRule {
        kind: PiiKind::MacAddress,
        regex: Regex::new(r"\b(?:[0-9A-Fa-f]{2}[:\-]){5}[0-9A-Fa-f]{2}\b").expect("mac regex"),
        validator: None,
        replacement: ReplacementMode::Plain,
    });

    rules
}

fn validate_china_id_card(value: &str) -> bool {
    let trimmed = value.trim();
    match trimmed.len() {
        18 => validate_china_id_18(trimmed),
        15 => trimmed.chars().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

fn validate_china_id_18(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 18 {
        return false;
    }
    if !bytes[..17].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let last = bytes[17].to_ascii_uppercase();
    if !(last.is_ascii_digit() || last == b'X') {
        return false;
    }
    let weights: [u32; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    let mut sum: u32 = 0;
    for (i, w) in weights.iter().enumerate() {
        let digit = (bytes[i] - b'0') as u32;
        sum += digit * w;
    }
    let expected = match sum % 11 {
        0 => b'1',
        1 => b'0',
        2 => b'X',
        3 => b'9',
        4 => b'8',
        5 => b'7',
        6 => b'6',
        7 => b'5',
        8 => b'4',
        9 => b'3',
        10 => b'2',
        _ => return false,
    };
    last == expected
}

fn validate_luhn(value: &str) -> bool {
    let digits: Vec<u32> = value
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum: u32 = 0;
    let parity = digits.len() % 2;
    for (idx, digit) in digits.iter().enumerate() {
        let mut d = *digit;
        if idx % 2 == parity {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    sum % 10 == 0
}

fn validate_non_loopback_ipv4(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let octets: Option<Vec<u8>> = parts.iter().map(|p| p.parse::<u8>().ok()).collect();
    let Some(octets) = octets else {
        return false;
    };
    if octets[0] == 0 {
        return false;
    }
    if octets[0] == 127 {
        return false;
    }
    if octets == [255, 255, 255, 255] {
        return false;
    }
    if octets[0] >= 224 {
        return false;
    }
    true
}

static GLOBAL_SANITIZER: OnceLock<Arc<PiiSanitizer>> = OnceLock::new();

pub fn global_sanitizer() -> Arc<PiiSanitizer> {
    GLOBAL_SANITIZER
        .get_or_init(|| {
            let cfg = load_persisted_config().unwrap_or_default();
            Arc::new(PiiSanitizer::new(cfg))
        })
        .clone()
}

pub fn update_global_config(config: PiiSanitizerConfig) {
    let sanitizer = global_sanitizer();
    sanitizer.update_config(config.clone());
    if let Err(err) = persist_config(&config) {
        tracing::debug!(target: "pii_sanitizer", error = %err, "failed to persist pii sanitizer config");
    }
}

fn pii_sanitizer_dir() -> Option<std::path::PathBuf> {
    let home = home_dir_for_config()?;
    Some(home.join(".senagentos"))
}

fn pii_sanitizer_config_path() -> Option<std::path::PathBuf> {
    pii_sanitizer_dir().map(|p| p.join("pii-sanitizer.json"))
}

fn home_dir_for_config() -> Option<std::path::PathBuf> {
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(std::path::PathBuf::from)
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
    }
}

fn load_persisted_config() -> Option<PiiSanitizerConfig> {
    let path = pii_sanitizer_config_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(PiiSanitizerConfig::from_settings(&value))
}

fn persist_config(config: &PiiSanitizerConfig) -> std::io::Result<()> {
    let Some(dir) = pii_sanitizer_dir() else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    let Some(path) = pii_sanitizer_config_path() else {
        return Ok(());
    };
    let disabled_labels: Vec<String> = config
        .disabled_kinds
        .iter()
        .map(|k| k.label().to_string())
        .collect();
    let payload = serde_json::json!({
        "enabled": config.enabled,
        "disabledKinds": disabled_labels,
    });
    let bytes = serde_json::to_vec_pretty(&payload).unwrap_or_else(|_| b"{}".to_vec());
    std::fs::write(path, bytes)
}

pub fn sanitize_text(input: &str) -> (String, SanitizationReport) {
    global_sanitizer().sanitize(input)
}

pub fn sanitize_text_in_place(input: &str) -> String {
    let (cleaned, _) = sanitize_text(input);
    cleaned
}

pub fn sanitize_json(value: &serde_json::Value) -> (serde_json::Value, SanitizationReport) {
    global_sanitizer().sanitize_json(value)
}
