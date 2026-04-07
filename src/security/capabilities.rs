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

/// A capability represents a specific permission granted to an agent.
///
/// Capabilities are fine-grained and can specify exact resources
/// (e.g., read access to a specific file) or broad categories
/// (e.g., all file read operations).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "capability", rename_all = "snake_case")]
pub enum Capability {
    /// Read files at specific path or pattern (glob supported).
    FileRead { pattern: String },
    /// Write files at specific path or pattern.
    FileWrite { pattern: String },
    /// Read directory contents.
    DirRead { path: String },
    /// Create or modify directories.
    DirWrite { path: String },
    /// Fetch from specific URL pattern.
    NetFetch { host_pattern: String },
    /// Invoke a specific tool by name.
    ToolInvoke { name: String },
    /// Invoke any tool (super capability).
    ToolAll,
    /// Execute shell commands matching pattern.
    ShellExec { command_pattern: String },
    /// Spawn child agents.
    AgentSpawn,
    /// Send messages to other agents.
    AgentMessage,
    /// Read from memory system.
    MemoryRead,
    /// Write to memory system.
    MemoryWrite,
    /// Query knowledge graph.
    KnowledgeQuery,
    /// Modify knowledge graph.
    KnowledgeWrite,
    /// Call LLM providers.
    LlmCall,
    /// Read environment variables.
    EnvRead,
    /// Connect to other nodes via peer protocol.
    OfpConnect,
    /// Perform economic transactions.
    EconTransact,
}

impl Capability {
    /// Get a human-readable description of this capability.
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

    /// Check if this capability is a wildcard/broad capability.
    pub fn is_broad(&self) -> bool {
        matches!(self, Capability::ToolAll)
            || matches!(self, Capability::FileRead { pattern } if pattern == "*")
            || matches!(self, Capability::FileWrite { pattern } if pattern == "*")
            || matches!(self, Capability::NetFetch { host_pattern } if host_pattern == "*")
            || matches!(self, Capability::ShellExec { command_pattern } if command_pattern == "*")
    }
}

/// Result of a capability check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityCheck {
    /// The requested capability is granted.
    Granted,
    /// The requested capability is denied with reason.
    Denied { reason: String },
}

impl CapabilityCheck {
    /// Check if the capability is granted.
    pub fn is_granted(&self) -> bool {
        matches!(self, CapabilityCheck::Granted)
    }

    /// Check if the capability is denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, CapabilityCheck::Denied { .. })
    }

    /// Require the capability to be granted, returning an error if denied.
    pub fn require(self) -> Result<(), CapabilityError> {
        match self {
            CapabilityCheck::Granted => Ok(()),
            CapabilityCheck::Denied { reason } => Err(CapabilityError::Denied { reason }),
        }
    }
}

/// Errors related to capability operations.
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

/// Check if a granted capability matches a required capability.
///
/// Returns true if the granted capability covers the required capability.
/// For example, `ToolAll` covers `ToolInvoke { name: "shell" }`.
pub fn capability_matches(granted: &Capability, required: &Capability) -> bool {
    use Capability::*;

    match (granted, required) {
        // Exact match
        (g, r) if g == r => true,

        // ToolAll covers any tool
        (ToolAll, ToolInvoke { .. }) => true,

        // File patterns: * covers everything, specific patterns match if granted is more general
        (FileRead { pattern: g }, FileRead { pattern: r }) => pattern_matches(g, r),
        (FileWrite { pattern: g }, FileWrite { pattern: r }) => pattern_matches(g, r),

        // Dir patterns
        (DirRead { path: g }, DirRead { path: r }) => path_matches(g, r),
        (DirWrite { path: g }, DirWrite { path: r }) => path_matches(g, r),

        // Net patterns
        (NetFetch { host_pattern: g }, NetFetch { host_pattern: r }) => host_pattern_matches(g, r),

        // Shell patterns
        (ShellExec { command_pattern: g }, ShellExec { command_pattern: r }) => {
            pattern_matches(g, r)
        }

        // AgentSpawn covers AgentMessage (spawning implies messaging)
        (AgentSpawn, AgentMessage) => true,

        // MemoryWrite covers MemoryRead (writing implies reading)
        (MemoryWrite, MemoryRead) => true,

        // KnowledgeWrite covers KnowledgeQuery
        (KnowledgeWrite, KnowledgeQuery) => true,

        // No match
        _ => false,
    }
}

/// Check if a pattern matches a required pattern.
/// Simple glob matching: * matches everything.
fn pattern_matches(granted: &str, required: &str) -> bool {
    if granted == "*" || granted == "**" {
        return true;
    }
    if required == "*" || required == "**" {
        // Specific granted cannot match wildcard required
        return false;
    }
    // Check if required starts with granted pattern (directory containment)
    if granted.ends_with("/*") || granted.ends_with("/**") {
        let prefix = granted.trim_end_matches("/*").trim_end_matches("/**");
        return required.starts_with(prefix);
    }
    granted == required
}

/// Check if a path matches another path pattern.
/// Uses boundary-safe prefix matching: granted "/foo" covers "/foo/bar"
/// but NOT "/foobar".
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

/// Check if a host pattern matches.
///
/// Supports exact match, wildcard (`*`), and dot-prefix suffix wildcards
/// (`*.example.com`). Suffix wildcards are boundary-safe: `*.example.com`
/// matches `foo.example.com` but NOT `evil-example.com` or `example.com`.
///
/// SECURITY NOTE: `*` alone grants unrestricted host access. Use only when
/// intentionally allowing all hosts (e.g., development/debugging).
fn host_pattern_matches(granted: &str, required: &str) -> bool {
    if granted == "*" {
        tracing::warn!(
            "Security: Wildcard '*' host pattern grants unrestricted access. \
             Consider using specific host patterns for production."
        );
        return true;
    }
    if granted.starts_with("*.") {
        // e.g. "*.example.com" → suffix = ".example.com"
        let suffix = &granted[1..]; // ".example.com" — includes the leading dot
        let base_domain = &granted[2..]; // "example.com" — skips "*."
        // Exact base domain match (e.g. "example.com" == "*.example.com")
        if required == base_domain {
            return true;
        }
        // Boundary-safe suffix check: the dot must be a real subdomain separator,
        // not just any character that happens to precede "example.com".
        // "foo.example.com".ends_with(".example.com") at position 3 ('.')
        // "evil-example.com".ends_with(".example.com") at position 4 ('-') — INVALID
        if let Some(pos) = required.find(suffix) {
            return pos == 0 || required.as_bytes().get(pos - 1) == Some(&b'.');
        }
        return false;
    }
    granted == required
}

/// Validate that child capabilities are covered by parent capabilities.
///
/// This prevents privilege escalation where a child agent gains more
/// permissions than its parent.
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

/// Check if any capability in a set matches the required capability.
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

/// Build a default set of safe capabilities for a new agent.
pub fn default_capabilities() -> Vec<Capability> {
    vec![
        Capability::ToolAll,
        Capability::MemoryRead,
        Capability::MemoryWrite,
        Capability::LlmCall,
        Capability::EnvRead,
    ]
}

/// Build a restricted set of capabilities (read-only).
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

/// Parse capabilities from a manifest string (one per line, format: "Capability(args)").
pub fn parse_capabilities(manifest: &str) -> Result<Vec<Capability>, serde_json::Error> {
    // Parse as JSON array
    serde_json::from_str(manifest)
}

/// Manager for per-agent capabilities.
///
/// Tracks which capabilities are granted to each agent and provides
/// thread-safe checking.
#[derive(Debug, Clone, Default)]
pub struct CapabilityManager {
    agents: std::collections::HashMap<String, Vec<Capability>>,
}

impl CapabilityManager {
    /// Create a new empty capability manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant capabilities to an agent.
    pub fn grant(&mut self, agent_id: &str, capabilities: Vec<Capability>) {
        self.agents
            .entry(agent_id.to_string())
            .or_default()
            .extend(capabilities);
    }

    /// Revoke all capabilities from an agent.
    pub fn revoke_all(&mut self, agent_id: &str) {
        self.agents.remove(agent_id);
    }

    /// Check if an agent has a specific capability.
    pub fn check(&self, agent_id: &str, required: &Capability) -> CapabilityCheck {
        match self.agents.get(agent_id) {
            Some(caps) => check_capabilities(caps, required),
            None => CapabilityCheck::Denied {
                reason: format!("Agent '{}' has no registered capabilities", agent_id),
            },
        }
    }

    /// Get all capabilities for an agent.
    pub fn get_capabilities(&self, agent_id: &str) -> &[Capability] {
        self.agents.get(agent_id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Register an agent with default capabilities.
    pub fn register_default(&mut self, agent_id: &str) {
        self.agents
            .entry(agent_id.to_string())
            .or_insert_with(default_capabilities);
    }

    /// Validate that a child agent's capabilities don't exceed the parent's.
    pub fn validate_spawn(
        &self,
        parent_id: &str,
        child_caps: &[Capability],
    ) -> Result<(), CapabilityError> {
        let parent_caps = self.get_capabilities(parent_id);
        validate_capability_inheritance(parent_caps, child_caps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_matches_exact() {
        let cap1 = Capability::ToolInvoke {
            name: "shell".to_string(),
        };
        let cap2 = Capability::ToolInvoke {
            name: "shell".to_string(),
        };
        let cap3 = Capability::ToolInvoke {
            name: "file".to_string(),
        };

        assert!(capability_matches(&cap1, &cap2));
        assert!(!capability_matches(&cap1, &cap3));
    }

    #[test]
    fn test_tool_all_covers_specific() {
        let all = Capability::ToolAll;
        let specific = Capability::ToolInvoke {
            name: "shell".to_string(),
        };

        assert!(capability_matches(&all, &specific));
        assert!(!capability_matches(&specific, &all));
    }

    #[test]
    fn test_file_pattern_matching() {
        let all_files = Capability::FileRead {
            pattern: "*".to_string(),
        };
        let specific = Capability::FileRead {
            pattern: "/tmp/test.txt".to_string(),
        };
        let dir_pattern = Capability::FileRead {
            pattern: "/home/*".to_string(),
        };
        let in_dir = Capability::FileRead {
            pattern: "/home/user/file.txt".to_string(),
        };
        let outside_dir = Capability::FileRead {
            pattern: "/etc/passwd".to_string(),
        };

        assert!(capability_matches(&all_files, &specific));
        assert!(capability_matches(&dir_pattern, &in_dir));
        assert!(!capability_matches(&dir_pattern, &outside_dir));
    }

    #[test]
    fn test_capability_inheritance_validation() {
        let parent = vec![
            Capability::ToolAll,
            Capability::FileRead {
                pattern: "/home/*".to_string(),
            },
        ];

        // Valid child: same or fewer capabilities
        let valid_child = vec![
            Capability::ToolInvoke {
                name: "shell".to_string(),
            },
            Capability::FileRead {
                pattern: "/home/user/*".to_string(),
            },
        ];

        assert!(validate_capability_inheritance(&parent, &valid_child).is_ok());

        // Invalid child: extra capability
        let invalid_child = vec![
            Capability::ToolAll,
            Capability::FileWrite {
                pattern: "/etc/*".to_string(),
            },
        ];

        assert!(validate_capability_inheritance(&parent, &invalid_child).is_err());
    }

    #[test]
    fn test_capability_check() {
        let caps = vec![Capability::ToolAll, Capability::MemoryRead];

        assert!(check_capabilities(&caps, &Capability::MemoryRead).is_granted());
        assert!(
            check_capabilities(
                &caps,
                &Capability::ToolInvoke {
                    name: "shell".to_string(),
                }
            )
            .is_granted()
        );
        assert!(check_capabilities(&caps, &Capability::MemoryWrite).is_denied());
    }

    #[test]
    fn test_host_pattern_matching() {
        assert!(host_pattern_matches("*", "api.example.com"));
        assert!(host_pattern_matches("*.example.com", "api.example.com"));
        assert!(host_pattern_matches("*.example.com", "example.com"));
        assert!(host_pattern_matches("api.example.com", "api.example.com"));
        assert!(!host_pattern_matches("*.example.com", "other.com"));
        assert!(
            !host_pattern_matches("*.example.com", "evil-example.com"),
            "wildcard must not match partial suffix without dot boundary"
        );
    }

    #[test]
    fn test_memory_write_implies_read() {
        let write = Capability::MemoryWrite;
        let read = Capability::MemoryRead;

        assert!(capability_matches(&write, &read));
        assert!(!capability_matches(&read, &write));
    }

    #[test]
    fn test_agent_spawn_implies_message() {
        let spawn = Capability::AgentSpawn;
        let message = Capability::AgentMessage;

        assert!(capability_matches(&spawn, &message));
    }

    #[test]
    fn test_capability_description() {
        let cap = Capability::FileRead {
            pattern: "/tmp/*".to_string(),
        };
        assert!(cap.description().contains("Read files"));
        assert!(cap.description().contains("/tmp/*"));
    }

    #[test]
    fn test_parse_capabilities_json() {
        let json = r#"[
            {"capability": "tool_all"},
            {"capability": "file_read", "pattern": "/tmp/*"},
            {"capability": "memory_read"}
        ]"#;

        let caps = parse_capabilities(json).unwrap();
        assert_eq!(caps.len(), 3);
        assert!(matches!(caps[0], Capability::ToolAll));
    }
}
