// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Capability-based security system for fine-grained per-agent permissions.
//!
//! This module provides a capability model:
//! - Fine-grained permissions per agent (file, network, tools, shell, etc.)
//! - Capability inheritance validation to prevent privilege escalation
//! - Runtime capability checking before sensitive operations
//!
//! Capabilities are granted to agents at spawn time and checked before
//! tool execution or sensitive operations.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "capability", rename_all = "snake_case")]
pub enum Capability {

    FileRead { pattern: String },

    FileWrite { pattern: String },

    DirRead { path: String },

    DirWrite { path: String },

    NetFetch { host_pattern: String },

    ToolInvoke { name: String },

    ToolAll,

    ShellExec { command_pattern: String },

    AgentSpawn,

    AgentMessage,

    MemoryRead,

    MemoryWrite,

    KnowledgeQuery,

    KnowledgeWrite,

    LlmCall,

    EnvRead,

    OfpConnect,

    EconTransact,
}

impl Capability {

    pub fn description(&self) -> String {
        match self {
            Capability::FileRead { pattern } => format!("Read files matching: {}", pattern),
            Capability::FileWrite { pattern } => format!("Write files matching: {}", pattern),
            Capability::DirRead { path } => format!("Read directory: {}", path),
            Capability::DirWrite { path } => format!("Write directory: {}", path),
            Capability::NetFetch { host_pattern } => {
                format!("Fetch from hosts matching: {}", host_pattern)
            }
            Capability::ToolInvoke { name } => format!("Invoke tool: {}", name),
            Capability::ToolAll => "Invoke any tool".to_string(),
            Capability::ShellExec { command_pattern } => {
                format!("Execute shell commands matching: {}", command_pattern)
            }
            Capability::AgentSpawn => "Spawn child agents".to_string(),
            Capability::AgentMessage => "Send messages to agents".to_string(),
            Capability::MemoryRead => "Read from memory".to_string(),
            Capability::MemoryWrite => "Write to memory".to_string(),
            Capability::KnowledgeQuery => "Query knowledge graph".to_string(),
            Capability::KnowledgeWrite => "Modify knowledge graph".to_string(),
            Capability::LlmCall => "Call LLM providers".to_string(),
            Capability::EnvRead => "Read environment variables".to_string(),
            Capability::OfpConnect => "Connect to peer nodes".to_string(),
            Capability::EconTransact => "Perform transactions".to_string(),
        }
    }

    pub fn is_broad(&self) -> bool {
        matches!(self, Capability::ToolAll)
            || matches!(self, Capability::FileRead { pattern } if pattern == "*")
            || matches!(self, Capability::FileWrite { pattern } if pattern == "*")
            || matches!(self, Capability::NetFetch { host_pattern } if host_pattern == "*")
            || matches!(self, Capability::ShellExec { command_pattern } if command_pattern == "*")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityCheck {

    Granted,

    Denied { reason: String },
}

impl CapabilityCheck {

    pub fn is_granted(&self) -> bool {
        matches!(self, CapabilityCheck::Granted)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, CapabilityCheck::Denied { .. })
    }

    pub fn require(self) -> Result<(), CapabilityError> {
        match self {
            CapabilityCheck::Granted => Ok(()),
            CapabilityCheck::Denied { reason } => Err(CapabilityError::Denied { reason }),
        }
    }
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CapabilityError {
    #[error("Capability denied: {reason}")]
    Denied { reason: String },
    #[error("Invalid capability pattern: {pattern}")]
    InvalidPattern { pattern: String },
    #[error(
        "Capability inheritance violation: child cannot have {child_capability} without parent having {parent_capability}"
    )]
    InheritanceViolation {
        child_capability: String,
        parent_capability: String,
    },
}

pub fn capability_matches(granted: &Capability, required: &Capability) -> bool {
    use Capability::*;

    match (granted, required) {

        (g, r) if g == r => true,

        (ToolAll, ToolInvoke { .. }) => true,

        (FileRead { pattern: g }, FileRead { pattern: r }) => pattern_matches(g, r),
        (FileWrite { pattern: g }, FileWrite { pattern: r }) => pattern_matches(g, r),

        (DirRead { path: g }, DirRead { path: r }) => path_matches(g, r),
        (DirWrite { path: g }, DirWrite { path: r }) => path_matches(g, r),

        (NetFetch { host_pattern: g }, NetFetch { host_pattern: r }) => host_pattern_matches(g, r),

        (ShellExec { command_pattern: g }, ShellExec { command_pattern: r }) => {
            pattern_matches(g, r)
        }

        (AgentSpawn, AgentMessage) => true,

        (MemoryWrite, MemoryRead) => true,

        (KnowledgeWrite, KnowledgeQuery) => true,

        _ => false,
    }
}

fn pattern_matches(granted: &str, required: &str) -> bool {
    if granted == "*" || granted == "**" {
        return true;
    }
    if required == "*" || required == "**" {

        return false;
    }

    if granted.ends_with("/*") || granted.ends_with("/**") {
        let prefix = granted.trim_end_matches("/*").trim_end_matches("/**");
        return required.starts_with(prefix);
    }
    granted == required
}

fn path_matches(granted: &str, required: &str) -> bool {
    if granted == "*" || granted == "/" {
        return true;
    }
    let prefix = granted.trim_end_matches('/');
    if required == prefix {
        return true;
    }
    required.starts_with(&format!("{prefix}/"))
}

fn host_pattern_matches(granted: &str, required: &str) -> bool {
    if granted == "*" {
        tracing::warn!(
            "Security: Wildcard '*' host pattern grants unrestricted access. \
             Consider using specific host patterns for production."
        );
        return true;
    }
    if granted.starts_with("*.") {

        let suffix = &granted[1..];
        let base_domain = &granted[2..];

        if required == base_domain {
            return true;
        }

        if let Some(pos) = required.find(suffix) {
            return pos == 0 || required.as_bytes().get(pos - 1) == Some(&b'.');
        }
        return false;
    }
    granted == required
}

pub fn validate_capability_inheritance(
    parent_caps: &[Capability],
    child_caps: &[Capability],
) -> Result<(), CapabilityError> {
    for child_cap in child_caps {
        let mut covered = false;

        for parent_cap in parent_caps {
            if capability_matches(parent_cap, child_cap) {
                covered = true;
                break;
            }
        }

        if !covered {
            return Err(CapabilityError::InheritanceViolation {
                child_capability: format!("{:?}", child_cap),
                parent_capability: format!("none matching among {} parent caps", parent_caps.len()),
            });
        }
    }

    Ok(())
}

pub fn check_capabilities(granted: &[Capability], required: &Capability) -> CapabilityCheck {
    for cap in granted {
        if capability_matches(cap, required) {
            return CapabilityCheck::Granted;
        }
    }

    CapabilityCheck::Denied {
        reason: format!("No capability matching {:?} found in granted set", required),
    }
}

pub fn default_capabilities() -> Vec<Capability> {
    vec![
        Capability::ToolAll,
        Capability::MemoryRead,
        Capability::MemoryWrite,
        Capability::LlmCall,
        Capability::EnvRead,
    ]
}

pub fn readonly_capabilities() -> Vec<Capability> {
    vec![
        Capability::FileRead {
            pattern: "*".to_string(),
        },
        Capability::DirRead {
            path: "/".to_string(),
        },
        Capability::MemoryRead,
        Capability::LlmCall,
        Capability::EnvRead,
    ]
}

pub fn parse_capabilities(manifest: &str) -> Result<Vec<Capability>, serde_json::Error> {

    serde_json::from_str(manifest)
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityManager {
    agents: std::collections::HashMap<String, Vec<Capability>>,
}

impl CapabilityManager {

    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(&mut self, agent_id: &str, capabilities: Vec<Capability>) {
        self.agents
            .entry(agent_id.to_string())
            .or_default()
            .extend(capabilities);
    }

    pub fn revoke_all(&mut self, agent_id: &str) {
        self.agents.remove(agent_id);
    }

    pub fn check(&self, agent_id: &str, required: &Capability) -> CapabilityCheck {
        match self.agents.get(agent_id) {
            Some(caps) => check_capabilities(caps, required),
            None => CapabilityCheck::Denied {
                reason: format!("Agent '{}' has no registered capabilities", agent_id),
            },
        }
    }

    pub fn get_capabilities(&self, agent_id: &str) -> &[Capability] {
        self.agents.get(agent_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn register_default(&mut self, agent_id: &str) {
        self.agents
            .entry(agent_id.to_string())
            .or_insert_with(default_capabilities);
    }

    pub fn validate_spawn(
        &self,
        parent_id: &str,
        child_caps: &[Capability],
    ) -> Result<(), CapabilityError> {
        let parent_caps = self.get_capabilities(parent_id);
        validate_capability_inheritance(parent_caps, child_caps)
    }
}
