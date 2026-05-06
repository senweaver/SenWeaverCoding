// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! IAM-aware policy enforcement for Nevis role-to-permission mapping.
//!
//! Evaluates tool and workspace access based on Nevis roles using a
//! deny-by-default policy model. All policy decisions are audit-logged.

use super::nevis::NevisIdentity;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleMapping {

    pub nevis_role: String,

    pub sen_permissions: Vec<String>,

    #[serde(default)]
    pub workspace_access: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {

    Allow,

    Deny(String),
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PolicyDecision::Allow)
    }
}

#[derive(Debug, Clone)]
pub struct IamPolicy {

    role_map: HashMap<String, CompiledRole>,
}

#[derive(Debug, Clone)]
struct CompiledRole {

    all_tools: bool,

    allowed_tools: Vec<String>,

    all_workspaces: bool,

    allowed_workspaces: Vec<String>,
}

impl IamPolicy {

    pub fn from_mappings(mappings: &[RoleMapping]) -> Result<Self> {
        let mut role_map = HashMap::new();

        for mapping in mappings {
            let key = mapping.nevis_role.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }

            let all_tools = mapping
                .sen_permissions
                .iter()
                .any(|p| p.eq_ignore_ascii_case("all"));
            let allowed_tools: Vec<String> = mapping
                .sen_permissions
                .iter()
                .filter(|p| !p.eq_ignore_ascii_case("all"))
                .map(|p| p.trim().to_ascii_lowercase())
                .collect();

            let all_workspaces = mapping
                .workspace_access
                .iter()
                .any(|w| w.eq_ignore_ascii_case("all"));
            let allowed_workspaces: Vec<String> = mapping
                .workspace_access
                .iter()
                .filter(|w| !w.eq_ignore_ascii_case("all"))
                .map(|w| w.trim().to_ascii_lowercase())
                .collect();

            if role_map.contains_key(&key) {
                bail!(
                    "IAM policy: duplicate role mapping for normalized key '{}' \
                     (from nevis_role '{}') — remove or merge the duplicate entry",
                    key,
                    mapping.nevis_role
                );
            }

            role_map.insert(
                key,
                CompiledRole {
                    all_tools,
                    allowed_tools,
                    all_workspaces,
                    allowed_workspaces,
                },
            );
        }

        Ok(Self { role_map })
    }

    pub fn evaluate_tool_access(
        &self,
        identity: &NevisIdentity,
        tool_name: &str,
    ) -> PolicyDecision {
        let normalized_tool = tool_name.trim().to_ascii_lowercase();
        if normalized_tool.is_empty() {
            return PolicyDecision::Deny("empty tool name".into());
        }

        for role in &identity.roles {
            let key = role.trim().to_ascii_lowercase();
            if let Some(compiled) = self.role_map.get(&key) {
                if compiled.all_tools
                    || compiled.allowed_tools.iter().any(|t| t == &normalized_tool)
                {
                    tracing::info!(
                        user_id = %crate::security::redact(&identity.user_id),
                        role = %key,
                        tool = %normalized_tool,
                        "IAM policy: tool access ALLOWED"
                    );
                    return PolicyDecision::Allow;
                }
            }
        }

        let reason = format!(
            "no role grants access to tool '{normalized_tool}' for user '{}'",
            crate::security::redact(&identity.user_id)
        );
        tracing::info!(
            user_id = %crate::security::redact(&identity.user_id),
            tool = %normalized_tool,
            "IAM policy: tool access DENIED"
        );
        PolicyDecision::Deny(reason)
    }

    pub fn evaluate_workspace_access(
        &self,
        identity: &NevisIdentity,
        workspace: &str,
    ) -> PolicyDecision {
        let normalized_ws = workspace.trim().to_ascii_lowercase();
        if normalized_ws.is_empty() {
            return PolicyDecision::Deny("empty workspace name".into());
        }

        for role in &identity.roles {
            let key = role.trim().to_ascii_lowercase();
            if let Some(compiled) = self.role_map.get(&key) {
                if compiled.all_workspaces
                    || compiled
                        .allowed_workspaces
                        .iter()
                        .any(|w| w == &normalized_ws)
                {
                    tracing::info!(
                        user_id = %crate::security::redact(&identity.user_id),
                        role = %key,
                        workspace = %normalized_ws,
                        "IAM policy: workspace access ALLOWED"
                    );
                    return PolicyDecision::Allow;
                }
            }
        }

        let reason = format!(
            "no role grants access to workspace '{normalized_ws}' for user '{}'",
            crate::security::redact(&identity.user_id)
        );
        tracing::info!(
            user_id = %crate::security::redact(&identity.user_id),
            workspace = %normalized_ws,
            "IAM policy: workspace access DENIED"
        );
        PolicyDecision::Deny(reason)
    }

    pub fn is_empty(&self) -> bool {
        self.role_map.is_empty()
    }
}
