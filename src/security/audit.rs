// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::config::AuditConfig;
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

const GENESIS_PREV_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

static GLOBAL_AUDIT: OnceLock<Arc<AuditLogger>> = OnceLock::new();

pub fn global_audit_logger() -> Option<&'static Arc<AuditLogger>> {
    if let Some(existing) = GLOBAL_AUDIT.get() {
        return Some(existing);
    }
    let svc = crate::services::try_get_services()?;
    let cfg = svc.config();
    if !cfg.security.audit.enabled {
        return None;
    }
    let sen_dir = cfg
        .config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let logger = AuditLogger::new(cfg.security.audit.clone(), sen_dir).ok()?;
    let _ = GLOBAL_AUDIT.set(Arc::new(logger));
    GLOBAL_AUDIT.get()
}

pub fn record_command_execution(
    channel: &str,
    command: &str,
    risk_level: &str,
    approved: bool,
    allowed: bool,
    success: bool,
    duration_ms: u64,
) {
    if let Some(logger) = global_audit_logger() {
        if let Err(e) = logger.log_command(
            channel,
            command,
            risk_level,
            approved,
            allowed,
            success,
            duration_ms,
        ) {
            tracing::debug!(error = %e, "audit log_command failed");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    CommandExecution,
    FileAccess,
    ConfigChange,
    AuthSuccess,
    AuthFailure,
    PolicyViolation,
    SecurityEvent,

    TrustRegression,

    PidVerificationFailure,

    SecretRedacted,

    GatewayRemoteAuth,

    SchedulerRejection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub channel: String,
    pub user_id: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub command: Option<String>,
    pub risk_level: Option<String>,
    pub approved: bool,
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityContext {
    pub policy_violation: bool,
    pub rate_limit_remaining: Option<u32>,
    pub sandbox_backend: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub event_id: String,
    pub event_type: AuditEventType,
    pub actor: Option<Actor>,
    pub action: Option<Action>,
    pub result: Option<ExecutionResult>,
    pub security: SecurityContext,

    #[serde(default)]
    pub sequence: u64,

    #[serde(default)]
    pub prev_hash: String,

    #[serde(default)]
    pub entry_hash: String,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature: Option<String>,
}

impl AuditEvent {

    pub fn new(event_type: AuditEventType) -> Self {
        Self {
            timestamp: Utc::now(),
            event_id: Uuid::new_v4().to_string(),
            event_type,
            actor: None,
            action: None,
            result: None,
            security: SecurityContext {
                policy_violation: false,
                rate_limit_remaining: None,
                sandbox_backend: None,
            },
            sequence: 0,
            prev_hash: String::new(),
            entry_hash: String::new(),
            signature: None,
        }
    }

    pub fn with_actor(
        mut self,
        channel: String,
        user_id: Option<String>,
        username: Option<String>,
    ) -> Self {
        self.actor = Some(Actor {
            channel,
            user_id,
            username,
        });
        self
    }

    pub fn with_action(
        mut self,
        command: String,
        risk_level: String,
        approved: bool,
        allowed: bool,
    ) -> Self {
        self.action = Some(Action {
            command: Some(command),
            risk_level: Some(risk_level),
            approved,
            allowed,
        });
        self
    }

    pub fn with_result(
        mut self,
        success: bool,
        exit_code: Option<i32>,
        duration_ms: u64,
        error: Option<String>,
    ) -> Self {
        self.result = Some(ExecutionResult {
            success,
            exit_code,
            duration_ms: Some(duration_ms),
            error,
        });
        self
    }

    pub fn with_security(mut self, sandbox_backend: Option<String>) -> Self {
        self.security.sandbox_backend = sandbox_backend;
        self
    }
}

fn compute_entry_hash(prev_hash: &str, event: &AuditEvent) -> String {

    let content = serde_json::json!({
        "timestamp": event.timestamp,
        "event_id": event.event_id,
        "event_type": event.event_type,
        "actor": event.actor,
        "action": event.action,
        "result": event.result,
        "security": event.security,
        "sequence": event.sequence,
    });
    let content_json = serde_json::to_string(&content).expect("serialize canonical content");

    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(content_json.as_bytes());
    hex::encode(hasher.finalize())
}

struct ChainState {
    prev_hash: String,
    sequence: u64,
}

pub struct AuditLogger {
    log_path: PathBuf,
    config: AuditConfig,
    chain: Mutex<ChainState>,

    signing_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct CommandExecutionLog<'a> {
    pub channel: &'a str,
    pub command: &'a str,
    pub risk_level: &'a str,
    pub approved: bool,
    pub allowed: bool,
    pub success: bool,
    pub duration_ms: u64,
}

impl AuditLogger {

    pub fn new(config: AuditConfig, sen_dir: PathBuf) -> Result<Self> {

        let signing_key = if config.sign_events {
            let key_hex = std::env::var("SEN_AUDIT_SIGNING_KEY").map_err(|_| {
                anyhow::anyhow!("sign_events enabled but SEN_AUDIT_SIGNING_KEY not set")
            })?;

            let key_bytes = hex::decode(&key_hex)
                .map_err(|_| anyhow::anyhow!("SEN_AUDIT_SIGNING_KEY must be hex-encoded"))?;

            if key_bytes.len() != 32 {
                bail!(
                    "SEN_AUDIT_SIGNING_KEY must be 32 bytes (64 hex chars), got {}",
                    key_bytes.len()
                );
            }

            Some(key_bytes)
        } else {
            None
        };

        let log_path = sen_dir.join(&config.log_path);
        let chain_state = recover_chain_state(&log_path);
        Ok(Self {
            log_path,
            config,
            chain: Mutex::new(chain_state),
            signing_key,
        })
    }

    fn compute_signature(&self, entry_hash: &str) -> Result<Option<String>> {
        if let Some(ref key_bytes) = self.signing_key {
            use hmac::{Hmac, Mac};
            use sha2::Sha256;

            let mut mac = Hmac::<Sha256>::new_from_slice(key_bytes)
                .map_err(|_| anyhow::anyhow!("Invalid HMAC key length"))?;
            mac.update(entry_hash.as_bytes());

            Ok(Some(hex::encode(mac.finalize().into_bytes())))
        } else {
            Ok(None)
        }
    }

    pub fn log(&self, event: &AuditEvent) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        self.rotate_if_needed()?;

        let mut chained = event.clone();
        {
            let mut state = self.chain.lock();
            chained.sequence = state.sequence;
            chained.prev_hash = state.prev_hash.clone();
            chained.entry_hash = compute_entry_hash(&state.prev_hash, &chained);

            chained.signature = self.compute_signature(&chained.entry_hash)?;

            state.prev_hash = chained.entry_hash.clone();
            state.sequence += 1;
        }

        let line = serde_json::to_string(&chained)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        writeln!(file, "{}", line)?;
        file.sync_all()?;

        Ok(())
    }

    pub fn log_command_event(&self, entry: CommandExecutionLog<'_>) -> Result<()> {
        let event = AuditEvent::new(AuditEventType::CommandExecution)
            .with_actor(entry.channel.to_string(), None, None)
            .with_action(
                entry.command.to_string(),
                entry.risk_level.to_string(),
                entry.approved,
                entry.allowed,
            )
            .with_result(entry.success, None, entry.duration_ms, None);

        self.log(&event)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_command(
        &self,
        channel: &str,
        command: &str,
        risk_level: &str,
        approved: bool,
        allowed: bool,
        success: bool,
        duration_ms: u64,
    ) -> Result<()> {
        self.log_command_event(CommandExecutionLog {
            channel,
            command,
            risk_level,
            approved,
            allowed,
            success,
            duration_ms,
        })
    }

    fn rotate_if_needed(&self) -> Result<()> {
        if let Ok(metadata) = std::fs::metadata(&self.log_path) {
            let current_size_mb = metadata.len() / (1024 * 1024);
            if current_size_mb >= u64::from(self.config.max_size_mb) {
                self.rotate()?;
            }
        }
        Ok(())
    }

    fn rotate(&self) -> Result<()> {
        for i in (1..10).rev() {
            let old_name = format!("{}.{}.log", self.log_path.display(), i);
            let new_name = format!("{}.{}.log", self.log_path.display(), i + 1);
            let _ = std::fs::rename(&old_name, &new_name);
        }

        let rotated = format!("{}.1.log", self.log_path.display());
        std::fs::rename(&self.log_path, &rotated)?;
        Ok(())
    }
}

fn recover_chain_state(log_path: &Path) -> ChainState {
    let file = match std::fs::File::open(log_path) {
        Ok(f) => f,
        Err(_) => {
            return ChainState {
                prev_hash: GENESIS_PREV_HASH.to_string(),
                sequence: 0,
            };
        }
    };

    let reader = BufReader::new(file);
    let mut last_entry: Option<AuditEvent> = None;
    for l in reader.lines().map_while(Result::ok) {
        if let Ok(entry) = serde_json::from_str::<AuditEvent>(&l) {
            last_entry = Some(entry);
        }
    }

    match last_entry {
        Some(entry) => ChainState {
            prev_hash: entry.entry_hash,
            sequence: entry.sequence + 1,
        },
        None => ChainState {
            prev_hash: GENESIS_PREV_HASH.to_string(),
            sequence: 0,
        },
    }
}

pub fn verify_chain(log_path: &Path) -> Result<u64> {
    let file = std::fs::File::open(log_path)?;
    let reader = BufReader::new(file);

    let mut expected_prev_hash = GENESIS_PREV_HASH.to_string();
    let mut expected_sequence: u64 = 0;

    let signing_key = std::env::var("SEN_AUDIT_SIGNING_KEY")
        .ok()
        .and_then(|key_hex| hex::decode(&key_hex).ok())
        .filter(|key_bytes| key_bytes.len() == 32);

    for (line_idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: AuditEvent = serde_json::from_str(&line)?;

        if entry.sequence != expected_sequence {
            bail!(
                "sequence gap at line {}: expected {}, got {}",
                line_idx + 1,
                expected_sequence,
                entry.sequence
            );
        }

        if entry.prev_hash != expected_prev_hash {
            bail!(
                "prev_hash mismatch at line {} (sequence {}): expected {}, got {}",
                line_idx + 1,
                entry.sequence,
                expected_prev_hash,
                entry.prev_hash
            );
        }

        let recomputed = compute_entry_hash(&entry.prev_hash, &entry);
        if entry.entry_hash != recomputed {
            bail!(
                "entry_hash mismatch at line {} (sequence {}): expected {}, got {}",
                line_idx + 1,
                entry.sequence,
                recomputed,
                entry.entry_hash
            );
        }

        if let Some(ref signature) = entry.signature {
            if let Some(ref key_bytes) = signing_key {
                use hmac::{Hmac, Mac};
                use sha2::Sha256;

                let mut mac = Hmac::<Sha256>::new_from_slice(key_bytes)
                    .map_err(|_| anyhow::anyhow!("Invalid HMAC key length during verification"))?;
                mac.update(entry.entry_hash.as_bytes());
                let expected_sig = hex::encode(mac.finalize().into_bytes());

                if signature != &expected_sig {
                    bail!(
                        "signature verification failed at line {} (sequence {}): signature mismatch",
                        line_idx + 1,
                        entry.sequence
                    );
                }
            }

        }

        expected_prev_hash = entry.entry_hash.clone();
        expected_sequence += 1;
    }

    Ok(expected_sequence)
}
