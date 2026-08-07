// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::{OtpConfig, OtpMethod};
use crate::security::pairing::constant_time_eq;
use crate::security::secrets::SecretStore;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use ring::hmac;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const OTP_SECRET_FILE: &str = "otp-secret";
const OTP_PAIRING_PIN_FILE: &str = "otp-pairing-pin";
const OTP_DIGITS: u32 = 6;
const OTP_ISSUER: &str = "SenWeaverCoding";

#[derive(Debug)]
pub struct OtpValidator {
    config: OtpConfig,
    secret: Vec<u8>,
    pairing_pin: Option<String>,
    cli_challenge: Mutex<Option<String>>,
    cached_codes: Mutex<HashMap<String, u64>>,
    failed_attempts: Mutex<u32>,
    locked_until: Mutex<u64>,
}

impl OtpValidator {
    pub fn from_config(
        config: &OtpConfig,
        sen_dir: &Path,
        store: &SecretStore,
    ) -> Result<(Self, Option<String>)> {
        match config.method {
            OtpMethod::Totp => Self::from_totp(config, sen_dir, store),
            OtpMethod::CliPrompt => Self::from_cli_prompt(config, sen_dir, store),
            OtpMethod::Pairing => Self::from_pairing(config, sen_dir, store),
        }
    }

    fn from_totp(
        config: &OtpConfig,
        sen_dir: &Path,
        store: &SecretStore,
    ) -> Result<(Self, Option<String>)> {
        let (secret, generated) = load_or_create_totp_secret(sen_dir, store)?;
        let validator = Self {
            config: config.clone(),
            secret,
            pairing_pin: None,
            cli_challenge: Mutex::new(None),
            cached_codes: Mutex::new(HashMap::new()),
            failed_attempts: Mutex::new(0),
            locked_until: Mutex::new(0),
        };
        let uri = if generated {
            Some(validator.otpauth_uri())
        } else {
            None
        };
        Ok((validator, uri))
    }

    fn from_cli_prompt(
        config: &OtpConfig,
        sen_dir: &Path,
        store: &SecretStore,
    ) -> Result<(Self, Option<String>)> {
        let (secret, _) = load_or_create_totp_secret(sen_dir, store)?;
        let challenge = generate_numeric_code();
        let validator = Self {
            config: config.clone(),
            secret,
            pairing_pin: None,
            cli_challenge: Mutex::new(Some(challenge.clone())),
            cached_codes: Mutex::new(HashMap::new()),
            failed_attempts: Mutex::new(0),
            locked_until: Mutex::new(0),
        };
        Ok((
            validator,
            Some(format!("CLI challenge code (one-time): {challenge}")),
        ))
    }

    fn from_pairing(
        config: &OtpConfig,
        sen_dir: &Path,
        store: &SecretStore,
    ) -> Result<(Self, Option<String>)> {
        let pin_path = sen_dir.join(OTP_PAIRING_PIN_FILE);
        let (pin, generated) = if pin_path.exists() {
            let encoded = fs::read_to_string(&pin_path).with_context(|| {
                format!("Failed to read OTP pairing pin {}", pin_path.display())
            })?;
            let decrypted = store
                .decrypt(encoded.trim())
                .context("Failed to decrypt OTP pairing pin")?;
            (decrypted, false)
        } else {
            let pin = generate_numeric_code();
            let encrypted = store
                .encrypt(&pin)
                .context("Failed to encrypt OTP pairing pin")?;
            write_secret_file(&pin_path, &encrypted)?;
            (pin, true)
        };
        let validator = Self {
            config: config.clone(),
            secret: Vec::new(),
            pairing_pin: Some(pin.clone()),
            cli_challenge: Mutex::new(None),
            cached_codes: Mutex::new(HashMap::new()),
            failed_attempts: Mutex::new(0),
            locked_until: Mutex::new(0),
        };
        let message = if generated {
            Some(format!("Initialized OTP pairing PIN: {pin}"))
        } else {
            None
        };
        Ok((validator, message))
    }

    pub fn validate(&self, code: &str) -> Result<bool> {
        self.validate_at(code, unix_timestamp_now())
    }

    fn validate_at(&self, code: &str, now_secs: u64) -> Result<bool> {
        let normalized = code.trim();
        if normalized.len() != OTP_DIGITS as usize
            || !normalized.chars().all(|ch| ch.is_ascii_digit())
        {
            return Ok(false);
        }

        {
            let locked_until = *self.locked_until.lock();
            if locked_until > now_secs {
                anyhow::bail!(
                    "OTP locked due to too many failed attempts; retry in {}s",
                    locked_until - now_secs
                );
            }
        }

        {
            let mut cache = self.cached_codes.lock();
            cache.retain(|_, expiry| *expiry >= now_secs);
            if cache
                .get(normalized)
                .is_some_and(|expiry| *expiry >= now_secs)
            {
                return Ok(true);
            }
        }

        let is_valid = match self.config.method {
            OtpMethod::Pairing => self
                .pairing_pin
                .as_deref()
                .is_some_and(|pin| constant_time_eq(normalized, pin.trim())),
            OtpMethod::CliPrompt => {
                let mut challenge = self.cli_challenge.lock();
                let challenge_ok = challenge
                    .as_deref()
                    .is_some_and(|c| constant_time_eq(normalized, c.trim()));
                if challenge_ok {
                    *challenge = None;
                }
                challenge_ok
            }
            OtpMethod::Totp => self.validate_totp_window(normalized, now_secs),
        };

        if is_valid {
            *self.failed_attempts.lock() = 0;
            let mut cache = self.cached_codes.lock();
            cache.insert(
                normalized.to_string(),
                now_secs.saturating_add(self.config.cache_valid_secs),
            );
            return Ok(true);
        }

        let mut failed = self.failed_attempts.lock();
        *failed = failed.saturating_add(1);
        let max_attempts = self.config.challenge_max_attempts.max(1);
        if *failed >= max_attempts {
            *self.locked_until.lock() = now_secs.saturating_add(300);
            *failed = 0;
        }
        Ok(false)
    }

    pub fn config(&self) -> &OtpConfig {
        &self.config
    }

    pub fn has_valid_cached_authorization(&self) -> bool {
        let now = unix_timestamp_now();
        let cache = self.cached_codes.lock();
        cache.values().any(|expiry| *expiry >= now)
    }

    pub fn issue_cli_challenge(&self) -> Option<String> {
        if !matches!(self.config.method, OtpMethod::CliPrompt) {
            return None;
        }
        let challenge = generate_numeric_code();
        *self.cli_challenge.lock() = Some(challenge.clone());
        Some(challenge)
    }

    fn validate_totp_window(&self, normalized: &str, now_secs: u64) -> bool {
        if self.secret.is_empty() {
            return false;
        }
        let step = self.config.token_ttl_secs.max(1);
        let counter = now_secs / step;
        let counters = [
            counter.saturating_sub(1),
            counter,
            counter.saturating_add(1),
        ];
        counters
            .iter()
            .map(|c| compute_totp_code(&self.secret, *c))
            .any(|candidate| constant_time_eq(&candidate, normalized))
    }

    pub fn otpauth_uri(&self) -> String {
        let secret = encode_base32_secret(&self.secret);
        let account = "sen";
        format!(
            "otpauth://totp/{issuer}:{account}?secret={secret}&issuer={issuer}&period={period}",
            issuer = OTP_ISSUER,
            period = self.config.token_ttl_secs.max(1)
        )
    }
}

static GLOBAL_OTP_GATE: std::sync::OnceLock<std::sync::Arc<OtpValidator>> =
    std::sync::OnceLock::new();

pub fn install_otp_gate(validator: OtpValidator) {
    let _ = GLOBAL_OTP_GATE.set(std::sync::Arc::new(validator));
}

pub fn global_otp_gate() -> Option<&'static std::sync::Arc<OtpValidator>> {
    GLOBAL_OTP_GATE.get()
}

pub fn action_requires_otp(action: &str) -> bool {
    let Some(gate) = global_otp_gate() else {
        return false;
    };
    if !gate.config.enabled {
        return false;
    }
    let needle = action.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    gate.config.gated_actions.iter().any(|a| {
        let a = a.trim().to_ascii_lowercase();
        a == "*" || a == needle || needle.starts_with(&format!("{a}_"))
    })
}

pub fn domain_requires_otp(host_or_url: &str) -> bool {
    let Some(gate) = global_otp_gate() else {
        return false;
    };
    if !gate.config.enabled || gate.config.gated_domains.is_empty() {
        return false;
    }
    let host = host_or_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    gate.config
        .gated_domains
        .iter()
        .any(|d| host == d.trim().to_ascii_lowercase() || host.ends_with(&format!(".{}", d.trim().to_ascii_lowercase())))
}

pub fn ensure_tool_allowed(tool_name: &str) -> Result<(), String> {
    if !action_requires_otp(tool_name) {
        return Ok(());
    }
    let Some(gate) = global_otp_gate() else {
        return Err(format!(
            "OTP required for tool '{tool_name}' but OTP gate is not initialized"
        ));
    };
    if gate.has_valid_cached_authorization() {
        return Ok(());
    }
    if let Ok(code) = std::env::var("SEN_OTP_CODE") {
        let trimmed = code.trim();
        if !trimmed.is_empty() {
            match gate.validate(trimmed) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    return Err(format!(
                        "OTP required for tool '{tool_name}'; SEN_OTP_CODE is invalid"
                    ));
                }
                Err(e) => return Err(format!("OTP required for tool '{tool_name}': {e}")),
            }
        }
    }
    if let Some(challenge) = gate.issue_cli_challenge() {
        return Err(format!(
            "OTP required for tool '{tool_name}'. CLI challenge: {challenge}. \
             Re-run with SEN_OTP_CODE set to the challenge code (or authenticator code for totp)."
        ));
    }
    Err(format!(
        "OTP required for tool '{tool_name}'. Set SEN_OTP_CODE to a valid OTP before retrying."
    ))
}

pub fn secret_file_path(sen_dir: &Path) -> PathBuf {
    sen_dir.join(OTP_SECRET_FILE)
}

fn load_or_create_totp_secret(sen_dir: &Path, store: &SecretStore) -> Result<(Vec<u8>, bool)> {
    let secret_path = secret_file_path(sen_dir);
    if secret_path.exists() {
        let encoded = fs::read_to_string(&secret_path).with_context(|| {
            format!("Failed to read OTP secret file {}", secret_path.display())
        })?;
        let decrypted = store
            .decrypt(encoded.trim())
            .context("Failed to decrypt OTP secret file")?;
        Ok((decode_base32_secret(&decrypted)?, false))
    } else {
        let raw: [u8; 20] = rand::random();
        let encoded_secret = encode_base32_secret(&raw);
        let encrypted = store
            .encrypt(&encoded_secret)
            .context("Failed to encrypt OTP secret")?;
        write_secret_file(&secret_path, &encrypted)?;
        Ok((raw.to_vec(), true))
    }
}

fn generate_numeric_code() -> String {
    let raw: u32 = rand::random::<u32>() % 1_000_000;
    format!("{raw:06}")
}

fn write_secret_file(path: &Path, value: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let temp_path = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    fs::write(&temp_path, value).with_context(|| {
        format!(
            "Failed to write temporary OTP secret {}",
            temp_path.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "Failed to atomically replace OTP secret file {}",
            path.display()
        )
    })?;
    Ok(())
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn compute_totp_code(secret: &[u8], counter: u64) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let counter_bytes = counter.to_be_bytes();
    let digest = hmac::sign(&key, &counter_bytes);
    let hash = digest.as_ref();

    let offset = (hash[19] & 0x0f) as usize;
    let binary = ((u32::from(hash[offset]) & 0x7f) << 24)
        | (u32::from(hash[offset + 1]) << 16)
        | (u32::from(hash[offset + 2]) << 8)
        | u32::from(hash[offset + 3]);

    let code = binary % 10_u32.pow(OTP_DIGITS);
    format!("{code:0>6}")
}

fn encode_base32_secret(input: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::new();
    let mut buffer = 0u32;
    let mut bits_left = 0u32;
    for &byte in input {
        buffer = (buffer << 8) | u32::from(byte);
        bits_left += 8;
        while bits_left >= 5 {
            bits_left -= 5;
            let index = ((buffer >> bits_left) & 0x1f) as usize;
            output.push(ALPHABET[index] as char);
        }
    }
    if bits_left > 0 {
        let index = ((buffer << (5 - bits_left)) & 0x1f) as usize;
        output.push(ALPHABET[index] as char);
    }
    output
}

fn decode_base32_secret(input: &str) -> Result<Vec<u8>> {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut buffer = 0u32;
    let mut bits_left = 0u32;
    let mut output = Vec::new();
    for ch in input.chars() {
        if ch == '=' || ch.is_whitespace() {
            continue;
        }
        let upper = ch.to_ascii_uppercase();
        let value = ALPHABET
            .iter()
            .position(|&c| c == upper as u8)
            .with_context(|| format!("Invalid base32 character '{ch}' in OTP secret"))?;
        buffer = (buffer << 5) | value as u32;
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            output.push(((buffer >> bits_left) & 0xff) as u8);
        }
    }
    Ok(output)
}
