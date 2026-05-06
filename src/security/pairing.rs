// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
// Gateway pairing mode — first-connect authentication.
//
// On startup the gateway generates a one-time pairing code printed to the
// terminal. The first client must present this code via `X-Pairing-Code`
// header on a `POST /pair` request. The server responds with a bearer token
// that must be sent on all subsequent requests via `Authorization: Bearer <token>`.
//
// Already-paired tokens are persisted in config so restarts don't require
// re-pairing.

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

const MAX_PAIR_ATTEMPTS: u32 = 5;

const PAIR_LOCKOUT_SECS: u64 = 300;

const MAX_TRACKED_CLIENTS: usize = 10_000;

const FAILED_ATTEMPT_RETENTION_SECS: u64 = 900;

const FAILED_ATTEMPT_SWEEP_INTERVAL_SECS: u64 = 300;

#[derive(Debug, Clone, Copy)]
struct FailedAttemptState {
    count: u32,
    lockout_until: Option<Instant>,
    last_attempt: Instant,
}

#[derive(Debug, Clone)]
pub struct PairingGuard {

    require_pairing: bool,

    pairing_code: Arc<Mutex<Option<String>>>,

    paired_tokens: Arc<Mutex<HashSet<String>>>,

    failed_attempts: Arc<Mutex<(HashMap<String, FailedAttemptState>, Instant)>>,
}

impl PairingGuard {

    pub fn new(require_pairing: bool, existing_tokens: &[String]) -> Self {
        let tokens: HashSet<String> = existing_tokens
            .iter()
            .map(|t| {
                if is_token_hash(t) {
                    t.clone()
                } else {
                    hash_token(t)
                }
            })
            .collect();
        let code = if require_pairing && tokens.is_empty() {
            Some(generate_code())
        } else {
            None
        };
        Self {
            require_pairing,
            pairing_code: Arc::new(Mutex::new(code)),
            paired_tokens: Arc::new(Mutex::new(tokens)),
            failed_attempts: Arc::new(Mutex::new((HashMap::new(), Instant::now()))),
        }
    }

    pub fn pairing_code(&self) -> Option<String> {
        self.pairing_code.lock().clone()
    }

    pub fn require_pairing(&self) -> bool {
        self.require_pairing
    }

    fn try_pair_blocking(&self, code: &str, client_id: &str) -> Result<Option<String>, u64> {
        let client_id = normalize_client_key(client_id);
        let now = Instant::now();

        {
            let mut guard = self.failed_attempts.lock();
            let (ref mut map, ref mut last_sweep) = *guard;

            if now.duration_since(*last_sweep).as_secs() >= FAILED_ATTEMPT_SWEEP_INTERVAL_SECS {
                prune_failed_attempts(map, now);
                *last_sweep = now;
            }

            if let Some(state) = map.get(&client_id) {
                if let Some(until) = state.lockout_until {
                    if now < until {
                        let remaining = (until - now).as_secs();
                        return Err(remaining.max(1));
                    }

                    map.remove(&client_id);
                }
            }
        }

        {
            let mut pairing_code = self.pairing_code.lock();
            if let Some(ref expected) = *pairing_code {
                if constant_time_eq(code.trim(), expected.trim()) {

                    {
                        let mut guard = self.failed_attempts.lock();
                        guard.0.remove(&client_id);
                    }
                    let token = generate_token();
                    let mut tokens = self.paired_tokens.lock();
                    tokens.insert(hash_token(&token));

                    *pairing_code = None;

                    return Ok(Some(token));
                }
            }
        }

        {
            let mut guard = self.failed_attempts.lock();
            let (ref mut map, _) = *guard;

            if map.len() >= MAX_TRACKED_CLIENTS {
                prune_failed_attempts(map, now);
            }
            if map.len() >= MAX_TRACKED_CLIENTS {

                if let Some(lru_key) = map
                    .iter()
                    .min_by_key(|(_, s)| s.last_attempt)
                    .map(|(k, _)| k.clone())
                {
                    map.remove(&lru_key);
                }
            }

            let entry = map.entry(client_id).or_insert(FailedAttemptState {
                count: 0,
                lockout_until: None,
                last_attempt: now,
            });

            entry.last_attempt = now;
            entry.count += 1;

            if entry.count >= MAX_PAIR_ATTEMPTS {
                entry.lockout_until = Some(now + std::time::Duration::from_secs(PAIR_LOCKOUT_SECS));
            }
        }

        Ok(None)
    }

    pub async fn try_pair(&self, code: &str, client_id: &str) -> Result<Option<String>, u64> {

        let this = self.clone();
        let code = code.to_string();
        let client_id = client_id.to_string();
        let handle = tokio::task::spawn_blocking(move || this.try_pair_blocking(&code, &client_id));

        handle
            .await
            .expect("failed to spawn blocking task this should not happen")
    }

    pub fn is_authenticated(&self, token: &str) -> bool {
        if !self.require_pairing {
            return true;
        }
        let hashed = hash_token(token);
        let tokens = self.paired_tokens.lock();
        tokens.contains(&hashed)
    }

    pub fn is_paired(&self) -> bool {
        let tokens = self.paired_tokens.lock();
        !tokens.is_empty()
    }

    pub fn tokens(&self) -> Vec<String> {
        let tokens = self.paired_tokens.lock();
        tokens.iter().cloned().collect()
    }

    pub fn generate_new_pairing_code(&self) -> Option<String> {
        if !self.require_pairing {
            return None;
        }
        let new_code = generate_code();
        *self.pairing_code.lock() = Some(new_code.clone());
        Some(new_code)
    }

    pub fn token_hash(token: &str) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(token.as_bytes()))
    }

    pub fn authenticate_and_hash(&self, token: &str) -> Option<String> {
        if self.is_authenticated(token) {
            Some(Self::token_hash(token))
        } else {
            None
        }
    }
}

fn normalize_client_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

fn prune_failed_attempts(map: &mut HashMap<String, FailedAttemptState>, now: Instant) {
    map.retain(|_, state| {
        now.duration_since(state.last_attempt).as_secs() < FAILED_ATTEMPT_RETENTION_SECS
    });
}

fn generate_code() -> String {

    const UPPER_BOUND: u32 = 1_000_000;
    const REJECT_THRESHOLD: u32 = (u32::MAX / UPPER_BOUND) * UPPER_BOUND;

    loop {
        let uuid = uuid::Uuid::new_v4();
        let bytes = uuid.as_bytes();
        let raw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);

        if raw < REJECT_THRESHOLD {
            return format!("{:06}", raw % UPPER_BOUND);
        }
    }
}

fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    format!("zc_{}", hex::encode(bytes))
}

fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn is_token_hash(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

#[allow(clippy::needless_bitwise_bool)]
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();

    let len_diff = a.len() ^ b.len();

    let max_len = a.len().max(b.len());
    let mut byte_diff = 0u8;
    for i in 0..max_len {
        let x = *a.get(i).unwrap_or(&0);
        let y = *b.get(i).unwrap_or(&0);
        byte_diff |= x ^ y;
    }

    (len_diff == 0) & (byte_diff == 0)
}

pub fn is_public_bind(host: &str) -> bool {
    !matches!(
        host,
        "127.0.0.1" | "localhost" | "::1" | "[::1]" | "0:0:0:0:0:0:0:1"
    )
}
