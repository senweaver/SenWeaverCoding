// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Unified RBAC (Role-Based Access Control) engine for SenWeaverCoding.
//!
//! Wires together existing security components into a cohesive access control
//! system suitable for an agent operating system:
//!
//! - **CallerIdentity**: Unified identity representation from any source
//!   (Gateway JWT, Channel user, CLI operator, API key)
//! - **RbacEngine**: Central authorization engine combining IamPolicy +
//!   GuardrailsEngine + Capabilities
//! - **UserStore**: Persistent user/role management (file-backed)
//! - **AccessContext**: Request-scoped context propagated through the call chain

use super::capabilities::{Capability, check_capabilities};
use super::iam_policy::{IamPolicy, PolicyDecision, RoleMapping};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Unified caller identity from any authentication source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerIdentity {
    /// Unique user ID (from Nevis, channel, API key hash, or "cli-operator").
    pub user_id: String,
    /// Display name (optional).
    pub display_name: Option<String>,
    /// Assigned roles (e.g., "admin", "operator", "viewer").
    pub roles: Vec<String>,
    /// Authentication source.
    pub auth_source: AuthSource,
    /// Channel the request came from (if applicable).
    pub channel: Option<String>,
    /// Whether MFA was verified.
    pub mfa_verified: bool,
}

/// How the caller was authenticated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthSource {
    /// CLI direct operator (implicit admin).
    Cli,
    /// Gateway pairing token.
    PairingToken,
    /// Nevis/OIDC JWT.
    Nevis,
    /// Channel-specific auth (Telegram, Discord, etc.).
    Channel { platform: String },
    /// API key authentication.
    ApiKey,
    /// Anonymous (when auth is disabled).
    Anonymous,
}

impl CallerIdentity {
    /// Create CLI operator identity (implicit admin).
    pub fn cli_operator() -> Self {
        Self {
            user_id: "cli-operator".into(),
            display_name: Some("CLI Operator".into()),
            roles: vec!["admin".into()],
            auth_source: AuthSource::Cli,
            channel: None,
            mfa_verified: false,
        }
    }

    /// Create anonymous identity.
    pub fn anonymous() -> Self {
        Self {
            user_id: "anonymous".into(),
            display_name: None,
            roles: vec![],
            auth_source: AuthSource::Anonymous,
            channel: None,
            mfa_verified: false,
        }
    }

    /// Create identity from a channel message sender.
    pub fn from_channel(platform: &str, user_id: &str, display_name: Option<&str>) -> Self {
        Self {
            user_id: format!("{platform}:{user_id}"),
            display_name: display_name.map(String::from),
            roles: vec![],
            auth_source: AuthSource::Channel {
                platform: platform.into(),
            },
            channel: Some(platform.into()),
            mfa_verified: false,
        }
    }

    /// Gateway WebSocket / HTTP session after pairing (or equivalent) checks upstream.
    ///
    /// `user_id` is stable per session so RBAC user records can be keyed consistently.
    pub fn from_gateway_session(session_id: &str) -> Self {
        Self {
            user_id: format!("gateway-ws:{session_id}"),
            display_name: None,
            roles: vec![],
            auth_source: AuthSource::PairingToken,
            channel: Some("gateway".into()),
            mfa_verified: false,
        }
    }

    /// Create identity from a Nevis token validation result.
    pub fn from_nevis(nevis: &super::nevis::NevisIdentity) -> Self {
        Self {
            user_id: nevis.user_id.clone(),
            display_name: None,
            roles: nevis.roles.clone(),
            auth_source: AuthSource::Nevis,
            channel: None,
            mfa_verified: nevis.mfa_verified,
        }
    }

    /// Check if this identity has a specific role.
    pub fn has_role(&self, role: &str) -> bool {
        let normalized = role.trim().to_ascii_lowercase();
        self.roles
            .iter()
            .any(|r| r.trim().to_ascii_lowercase() == normalized)
    }

    /// Check if this identity is an admin.
    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }
}

/// Request-scoped access context propagated through the call chain.
///
/// This is the key integration type: created at the Gateway/Channel entry
/// point and threaded through to tool execution.
#[derive(Debug, Clone)]
pub struct AccessContext {
    /// The authenticated caller.
    pub identity: CallerIdentity,
    /// Resolved capabilities for this session.
    pub capabilities: Vec<Capability>,
    /// Active workspace (if any).
    pub workspace: Option<String>,
    /// Session ID for audit correlation.
    pub session_id: String,
}

impl AccessContext {
    pub fn new(identity: CallerIdentity) -> Self {
        let session_id = format!(
            "{}_{:x}",
            &identity.user_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        Self {
            identity,
            capabilities: vec![],
            workspace: None,
            session_id,
        }
    }

    /// Create a context for the CLI operator (full access).
    pub fn cli_operator() -> Self {
        let mut ctx = Self::new(CallerIdentity::cli_operator());
        ctx.capabilities = super::capabilities::default_capabilities();
        ctx
    }

    /// Check if the caller can use a specific tool.
    pub fn can_use_tool(&self, tool_name: &str) -> bool {
        if self.identity.is_admin() {
            return true;
        }
        let required = Capability::ToolInvoke {
            name: tool_name.to_string(),
        };
        check_capabilities(&self.capabilities, &required).is_granted()
    }
}

/// Built-in role definitions for the operating system.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoleDefinition {
    /// Role name (unique, case-insensitive).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Tools this role can access. Use "all" for unrestricted.
    pub allowed_tools: Vec<String>,
    /// Workspaces this role can access. Use "all" for unrestricted.
    #[serde(default)]
    pub allowed_workspaces: Vec<String>,
    /// Whether this is a built-in role (cannot be deleted).
    #[serde(default)]
    pub builtin: bool,
}

/// Persistent user record.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserRecord {
    /// Unique user ID.
    pub user_id: String,
    /// Display name.
    #[serde(default)]
    pub display_name: String,
    /// Assigned role names.
    pub roles: Vec<String>,
    /// Whether the user is active.
    #[serde(default = "default_true")]
    pub active: bool,
    /// Created timestamp (seconds since epoch).
    #[serde(default)]
    pub created_at: u64,
    /// Optional channel bindings (e.g., "telegram:12345").
    #[serde(default)]
    pub channel_bindings: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// RBAC configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RbacConfig {
    /// Enable RBAC enforcement. When false, all authenticated users get full access.
    #[serde(default)]
    pub enabled: bool,

    /// Default role for new/unknown users (empty = deny by default).
    #[serde(default = "default_role")]
    pub default_role: String,

    /// Grant CLI operators implicit admin (default: true).
    #[serde(default = "default_true")]
    pub cli_is_admin: bool,

    /// Grant pairing-token users this role (empty = use default_role).
    #[serde(default)]
    pub pairing_token_role: String,

    /// Path to the users file (relative to workspace or absolute).
    #[serde(default = "default_users_file")]
    pub users_file: String,

    /// Path to the roles file (relative to workspace or absolute).
    #[serde(default = "default_roles_file")]
    pub roles_file: String,
}

fn default_role() -> String {
    "viewer".into()
}
fn default_users_file() -> String {
    "users.json".into()
}
fn default_roles_file() -> String {
    "roles.json".into()
}

impl Default for RbacConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_role: default_role(),
            cli_is_admin: true,
            pairing_token_role: String::new(),
            users_file: default_users_file(),
            roles_file: default_roles_file(),
        }
    }
}

/// Central RBAC engine combining all access control mechanisms.
pub struct RbacEngine {
    config: RbacConfig,
    iam_policy: IamPolicy,
    roles: HashMap<String, RoleDefinition>,
    users: parking_lot::RwLock<HashMap<String, UserRecord>>,
    workspace_dir: PathBuf,
}

impl RbacEngine {
    /// Create a new RBAC engine with built-in roles.
    pub fn new(config: RbacConfig, workspace_dir: &Path) -> Self {
        let mut roles = HashMap::new();
        for role in builtin_roles() {
            roles.insert(role.name.to_ascii_lowercase(), role);
        }

        let mappings = Self::roles_to_mappings(&roles);
        let iam_policy = IamPolicy::from_mappings(&mappings).unwrap_or_else(|e| {
            tracing::warn!("RBAC: failed to compile IAM policy: {e}, using empty policy");
            IamPolicy::from_mappings(&[]).unwrap()
        });

        let mut engine = Self {
            config,
            iam_policy,
            roles,
            users: parking_lot::RwLock::new(HashMap::new()),
            workspace_dir: workspace_dir.to_path_buf(),
        };

        engine.load_custom_roles();
        engine.load_users();
        engine.rebuild_iam_policy();

        engine
    }

    /// Authorize a tool call for a given identity.
    ///
    /// This is the primary entry point called before tool execution.
    pub fn authorize_tool(
        &self,
        identity: &CallerIdentity,
        tool_name: &str,
    ) -> AuthorizationResult {
        if !self.config.enabled {
            return AuthorizationResult::allowed();
        }

        if identity.auth_source == AuthSource::Cli && self.config.cli_is_admin {
            return AuthorizationResult::allowed();
        }

        let effective_identity = self.resolve_effective_identity(identity);

        let nevis_id = super::nevis::NevisIdentity {
            user_id: effective_identity.user_id.clone(),
            roles: effective_identity.roles.clone(),
            scopes: vec![],
            mfa_verified: effective_identity.mfa_verified,
            session_expiry: u64::MAX,
        };

        let decision = self.iam_policy.evaluate_tool_access(&nevis_id, tool_name);

        match decision {
            PolicyDecision::Allow => AuthorizationResult::allowed(),
            PolicyDecision::Deny(reason) => AuthorizationResult::denied(reason),
        }
    }

    /// Resolve the effective identity by looking up stored user records
    /// and applying default roles.
    fn resolve_effective_identity(&self, identity: &CallerIdentity) -> CallerIdentity {
        let mut effective = identity.clone();

        let users = self.users.read();
        if let Some(user_record) = users.get(&identity.user_id) {
            if !user_record.active {
                effective.roles.clear();
                return effective;
            }
            effective.roles = user_record.roles.clone();
            if effective.display_name.is_none() && !user_record.display_name.is_empty() {
                effective.display_name = Some(user_record.display_name.clone());
            }
            return effective;
        }

        // Check channel bindings
        for (_, record) in users.iter() {
            if record.active
                && record
                    .channel_bindings
                    .iter()
                    .any(|b| b == &identity.user_id)
            {
                effective.roles = record.roles.clone();
                return effective;
            }
        }

        drop(users);

        if effective.roles.is_empty() {
            if identity.auth_source == AuthSource::PairingToken
                && !self.config.pairing_token_role.is_empty()
            {
                effective.roles = vec![self.config.pairing_token_role.clone()];
            } else if !self.config.default_role.is_empty() {
                effective.roles = vec![self.config.default_role.clone()];
            }
        }

        effective
    }

    /// Build an AccessContext for a caller.
    pub fn build_context(&self, identity: CallerIdentity) -> AccessContext {
        let effective = self.resolve_effective_identity(&identity);
        let capabilities = self.resolve_capabilities(&effective);
        AccessContext {
            identity: effective,
            capabilities,
            workspace: None,
            session_id: format!(
                "{}_{:x}",
                &identity.user_id,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
        }
    }

    fn resolve_capabilities(&self, identity: &CallerIdentity) -> Vec<Capability> {
        if identity.is_admin() {
            return vec![
                Capability::ToolAll,
                Capability::MemoryWrite,
                Capability::KnowledgeWrite,
                Capability::AgentSpawn,
                Capability::LlmCall,
                Capability::EnvRead,
            ];
        }

        let mut caps = Vec::new();
        for role_name in &identity.roles {
            let key = role_name.trim().to_ascii_lowercase();
            if let Some(role_def) = self.roles.get(&key) {
                if role_def
                    .allowed_tools
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case("all"))
                {
                    caps.push(Capability::ToolAll);
                } else {
                    for tool in &role_def.allowed_tools {
                        caps.push(Capability::ToolInvoke { name: tool.clone() });
                    }
                }
            }
        }
        // SECURITY: LlmCall and EnvRead are default capabilities.
        // This is intentionally permissive for backward compatibility.
        // Restrict these in production by configuring explicit roles.
        caps.push(Capability::LlmCall);
        caps.push(Capability::EnvRead);
        caps
    }

    // ── User Management (CRUD) ─────────────────────────────────────

    pub fn list_users(&self) -> Vec<UserRecord> {
        self.users.read().values().cloned().collect()
    }

    pub fn get_user(&self, user_id: &str) -> Option<UserRecord> {
        self.users.read().get(user_id).cloned()
    }

    pub fn create_user(&self, record: UserRecord) -> Result<(), String> {
        let mut users = self.users.write();
        if users.contains_key(&record.user_id) {
            return Err(format!("User '{}' already exists", record.user_id));
        }
        let mut record = record;
        if record.created_at == 0 {
            record.created_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
        }
        users.insert(record.user_id.clone(), record);
        drop(users);
        self.save_users();
        Ok(())
    }

    pub fn update_user(&self, record: UserRecord) -> Result<(), String> {
        let mut users = self.users.write();
        if !users.contains_key(&record.user_id) {
            return Err(format!("User '{}' not found", record.user_id));
        }
        users.insert(record.user_id.clone(), record);
        drop(users);
        self.save_users();
        Ok(())
    }

    pub fn delete_user(&self, user_id: &str) -> Result<(), String> {
        let mut users = self.users.write();
        if users.remove(user_id).is_none() {
            return Err(format!("User '{}' not found", user_id));
        }
        drop(users);
        self.save_users();
        Ok(())
    }

    // ── Role Management ────────────────────────────────────────────

    pub fn list_roles(&self) -> Vec<&RoleDefinition> {
        self.roles.values().collect()
    }

    pub fn get_role(&self, name: &str) -> Option<&RoleDefinition> {
        self.roles.get(&name.to_ascii_lowercase())
    }

    pub fn create_role(&mut self, role: RoleDefinition) -> Result<(), String> {
        let key = role.name.to_ascii_lowercase();
        if self.roles.contains_key(&key) {
            return Err(format!("Role '{}' already exists", role.name));
        }
        self.roles.insert(key, role);
        self.rebuild_iam_policy();
        self.save_roles();
        Ok(())
    }

    pub fn delete_role(&mut self, name: &str) -> Result<(), String> {
        let key = name.to_ascii_lowercase();
        if let Some(role) = self.roles.get(&key) {
            if role.builtin {
                return Err(format!("Cannot delete built-in role '{}'", name));
            }
        }
        if self.roles.remove(&key).is_none() {
            return Err(format!("Role '{}' not found", name));
        }
        self.rebuild_iam_policy();
        self.save_roles();
        Ok(())
    }

    // ── Internal helpers ───────────────────────────────────────────

    fn roles_to_mappings(roles: &HashMap<String, RoleDefinition>) -> Vec<RoleMapping> {
        roles
            .values()
            .map(|r| RoleMapping {
                nevis_role: r.name.clone(),
                sen_permissions: r.allowed_tools.clone(),
                workspace_access: r.allowed_workspaces.clone(),
            })
            .collect()
    }

    fn rebuild_iam_policy(&mut self) {
        let mappings = Self::roles_to_mappings(&self.roles);
        match IamPolicy::from_mappings(&mappings) {
            Ok(policy) => self.iam_policy = policy,
            Err(e) => tracing::warn!("RBAC: failed to rebuild IAM policy: {e}"),
        }
    }

    fn safe_workspace_path(&self, configured: &str, default_name: &str) -> PathBuf {
        let p = Path::new(configured);
        let candidate = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.workspace_dir.join(p)
        };

        match candidate.canonicalize() {
            Ok(canon) => {
                if let Ok(ws_canon) = self.workspace_dir.canonicalize() {
                    if canon.starts_with(&ws_canon) {
                        return canon;
                    }
                    tracing::warn!(
                        "RBAC: configured path {:?} escapes workspace; falling back to default",
                        configured,
                    );
                }
                self.workspace_dir.join(default_name)
            }
            Err(_) => {
                if let Ok(ws_canon) = self.workspace_dir.canonicalize() {
                    if let Ok(parent_canon) = candidate
                        .parent()
                        .unwrap_or(&self.workspace_dir)
                        .canonicalize()
                    {
                        if !parent_canon.starts_with(&ws_canon) {
                            tracing::warn!(
                                "RBAC: configured path {:?} escapes workspace; falling back to default",
                                configured,
                            );
                            return self.workspace_dir.join(default_name);
                        }
                    }
                }
                candidate
            }
        }
    }

    fn users_path(&self) -> PathBuf {
        self.safe_workspace_path(&self.config.users_file, "users.json")
    }

    fn roles_path(&self) -> PathBuf {
        self.safe_workspace_path(&self.config.roles_file, "roles.json")
    }

    fn load_users(&mut self) {
        let path = self.users_path();
        if !path.exists() {
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<Vec<UserRecord>>(&content) {
                Ok(records) => {
                    let mut users = self.users.write();
                    for record in records {
                        users.insert(record.user_id.clone(), record);
                    }
                    tracing::info!("RBAC: loaded {} users from {}", users.len(), path.display());
                }
                Err(e) => tracing::warn!("RBAC: failed to parse users file: {e}"),
            },
            Err(e) => tracing::warn!("RBAC: failed to read users file: {e}"),
        }
    }

    fn save_users(&self) {
        let path = self.users_path();
        let guard = self.users.read();
        let users: Vec<&UserRecord> = guard.values().collect();
        if let Ok(json) = serde_json::to_string_pretty(&users) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&path, json) {
                // SECURITY: Write failures are logged as errors, not just warnings.
                // In production, consider alerting on persistent write failures.
                tracing::error!(
                    error = %e,
                    path = %path.display(),
                    "RBAC: CRITICAL failed to save users. User permission changes may be lost on restart."
                );
            }
        }
    }

    fn load_custom_roles(&mut self) {
        let path = self.roles_path();
        if !path.exists() {
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<Vec<RoleDefinition>>(&content) {
                Ok(custom_roles) => {
                    for role in custom_roles {
                        let key = role.name.to_ascii_lowercase();
                        if !self.roles.contains_key(&key) {
                            self.roles.insert(key, role);
                        }
                    }
                    tracing::info!(
                        "RBAC: loaded roles from {}, total: {}",
                        path.display(),
                        self.roles.len()
                    );
                }
                Err(e) => tracing::warn!("RBAC: failed to parse roles file: {e}"),
            },
            Err(e) => tracing::warn!("RBAC: failed to read roles file: {e}"),
        }
    }

    fn save_roles(&self) {
        let path = self.roles_path();
        let custom_roles: Vec<&RoleDefinition> =
            self.roles.values().filter(|r| !r.builtin).collect();
        if custom_roles.is_empty() {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(&custom_roles) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("RBAC: failed to save roles: {e}");
            }
        }
    }
}

/// Result of an authorization check.
#[derive(Debug, Clone)]
pub struct AuthorizationResult {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl AuthorizationResult {
    pub fn allowed() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn denied(reason: String) -> Self {
        Self {
            allowed: false,
            reason: Some(reason),
        }
    }
}

/// Built-in role definitions for a typical agent operating system.
fn builtin_roles() -> Vec<RoleDefinition> {
    vec![
        RoleDefinition {
            name: "admin".into(),
            description:
                "Full system access. Can use all tools, manage users, and configure the system."
                    .into(),
            allowed_tools: vec!["all".into()],
            allowed_workspaces: vec!["all".into()],
            builtin: true,
        },
        RoleDefinition {
            name: "operator".into(),
            description: "Operational access. Can use most tools except security-sensitive ones."
                .into(),
            allowed_tools: vec![
                "shell".into(),
                "file_read".into(),
                "file_write".into(),
                "file_edit".into(),
                "notebook_edit".into(),
                "glob_search".into(),
                "content_search".into(),
                "dir_list".into(),
                "web_search".into(),
                "multi_search".into(),
                "web_fetch".into(),
                "youtube_search".into(),
                "github_search".into(),
                "reddit_search".into(),
                "image_search".into(),
                "text_browser".into(),
                "memory_store".into(),
                "memory_recall".into(),
                "git_operations".into(),
                "calculator".into(),
                "weather".into(),
                "delegate".into(),
                "llm_task".into(),
                "present_files".into(),
                "view_image".into(),
                "pdf_read".into(),
            ],
            allowed_workspaces: vec!["all".into()],
            builtin: true,
        },
        RoleDefinition {
            name: "developer".into(),
            description: "Developer access. File operations, search, and development tools.".into(),
            allowed_tools: vec![
                "shell".into(),
                "file_read".into(),
                "file_write".into(),
                "file_edit".into(),
                "notebook_edit".into(),
                "glob_search".into(),
                "content_search".into(),
                "dir_list".into(),
                "git_operations".into(),
                "web_search".into(),
                "web_fetch".into(),
                "github_search".into(),
                "calculator".into(),
                "memory_store".into(),
                "memory_recall".into(),
                "present_files".into(),
                "view_image".into(),
                "pdf_read".into(),
            ],
            allowed_workspaces: vec!["all".into()],
            builtin: true,
        },
        RoleDefinition {
            name: "analyst".into(),
            description: "Read and search access. Can search, browse, and analyze but not modify."
                .into(),
            allowed_tools: vec![
                "file_read".into(),
                "glob_search".into(),
                "content_search".into(),
                "dir_list".into(),
                "web_search".into(),
                "multi_search".into(),
                "web_fetch".into(),
                "youtube_search".into(),
                "github_search".into(),
                "reddit_search".into(),
                "image_search".into(),
                "text_browser".into(),
                "memory_recall".into(),
                "calculator".into(),
                "weather".into(),
                "present_files".into(),
                "view_image".into(),
                "pdf_read".into(),
            ],
            allowed_workspaces: vec!["all".into()],
            builtin: true,
        },
        RoleDefinition {
            name: "viewer".into(),
            description: "Read-only access. Can only read files and recall memory.".into(),
            allowed_tools: vec![
                "file_read".into(),
                "memory_recall".into(),
                "calculator".into(),
                "weather".into(),
                "present_files".into(),
            ],
            allowed_workspaces: vec![],
            builtin: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_engine() -> (RbacEngine, TempDir) {
        let tmp = TempDir::new().unwrap();
        let config = RbacConfig {
            enabled: true,
            ..Default::default()
        };
        (RbacEngine::new(config, tmp.path()), tmp)
    }

    #[test]
    fn cli_operator_is_admin() {
        let (engine, _tmp) = test_engine();
        let identity = CallerIdentity::cli_operator();
        let result = engine.authorize_tool(&identity, "shell");
        assert!(result.allowed);
    }

    #[test]
    fn admin_role_allows_everything() {
        let (engine, _tmp) = test_engine();
        let mut identity = CallerIdentity::anonymous();
        identity.roles = vec!["admin".into()];
        identity.auth_source = AuthSource::PairingToken;

        let result = engine.authorize_tool(&identity, "shell");
        assert!(result.allowed);
        let result = engine.authorize_tool(&identity, "any_tool");
        assert!(result.allowed);
    }

    #[test]
    fn viewer_denied_shell() {
        let (engine, _tmp) = test_engine();
        let mut identity = CallerIdentity::anonymous();
        identity.roles = vec!["viewer".into()];
        identity.auth_source = AuthSource::PairingToken;

        let result = engine.authorize_tool(&identity, "shell");
        assert!(!result.allowed);
    }

    #[test]
    fn viewer_allowed_file_read() {
        let (engine, _tmp) = test_engine();
        let mut identity = CallerIdentity::anonymous();
        identity.roles = vec!["viewer".into()];
        identity.auth_source = AuthSource::PairingToken;

        let result = engine.authorize_tool(&identity, "file_read");
        assert!(result.allowed);
    }

    #[test]
    fn default_role_applied() {
        let (engine, _tmp) = test_engine();
        let identity = CallerIdentity {
            user_id: "unknown-user".into(),
            display_name: None,
            roles: vec![],
            auth_source: AuthSource::PairingToken,
            channel: None,
            mfa_verified: false,
        };

        let result = engine.authorize_tool(&identity, "file_read");
        assert!(
            result.allowed,
            "default role (viewer) should allow file_read"
        );

        let result = engine.authorize_tool(&identity, "shell");
        assert!(!result.allowed, "default role (viewer) should deny shell");
    }

    #[test]
    fn disabled_rbac_allows_all() {
        let tmp = TempDir::new().unwrap();
        let config = RbacConfig {
            enabled: false,
            ..Default::default()
        };
        let engine = RbacEngine::new(config, tmp.path());
        let identity = CallerIdentity::anonymous();

        let result = engine.authorize_tool(&identity, "shell");
        assert!(result.allowed);
    }

    #[test]
    fn user_crud() {
        let (engine, _tmp) = test_engine();

        let user = UserRecord {
            user_id: "test-user".into(),
            display_name: "Test".into(),
            roles: vec!["operator".into()],
            active: true,
            created_at: 0,
            channel_bindings: vec![],
        };

        assert!(engine.create_user(user.clone()).is_ok());
        assert!(engine.create_user(user.clone()).is_err()); // duplicate

        let found = engine.get_user("test-user");
        assert!(found.is_some());
        assert_eq!(found.unwrap().roles, vec!["operator"]);

        assert!(engine.delete_user("test-user").is_ok());
        assert!(engine.get_user("test-user").is_none());
    }

    #[test]
    fn channel_binding_lookup() {
        let (engine, _tmp) = test_engine();

        let user = UserRecord {
            user_id: "john".into(),
            display_name: "John".into(),
            roles: vec!["operator".into()],
            active: true,
            created_at: 0,
            channel_bindings: vec!["telegram:12345".into()],
        };
        engine.create_user(user).unwrap();

        let identity = CallerIdentity::from_channel("telegram", "12345", Some("John"));
        let result = engine.authorize_tool(&identity, "shell");
        assert!(
            result.allowed,
            "telegram user bound to operator should access shell"
        );
    }

    #[test]
    fn inactive_user_denied() {
        let (engine, _tmp) = test_engine();

        let user = UserRecord {
            user_id: "suspended".into(),
            display_name: "Suspended".into(),
            roles: vec!["admin".into()],
            active: false,
            created_at: 0,
            channel_bindings: vec![],
        };
        engine.create_user(user).unwrap();

        let mut identity = CallerIdentity::anonymous();
        identity.user_id = "suspended".into();
        identity.auth_source = AuthSource::PairingToken;

        let result = engine.authorize_tool(&identity, "file_read");
        assert!(!result.allowed, "inactive user should be denied");
    }

    #[test]
    fn builtin_roles_present() {
        let (engine, _tmp) = test_engine();
        let roles = engine.list_roles();
        let names: Vec<&str> = roles.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"admin"));
        assert!(names.contains(&"operator"));
        assert!(names.contains(&"developer"));
        assert!(names.contains(&"analyst"));
        assert!(names.contains(&"viewer"));
    }

    #[test]
    fn cannot_delete_builtin_role() {
        let (mut engine, _tmp) = test_engine();
        let result = engine.delete_role("admin");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("built-in"));
    }

    #[test]
    fn access_context_tool_check() {
        let (engine, _tmp) = test_engine();
        let mut identity = CallerIdentity::anonymous();
        identity.roles = vec!["viewer".into()];
        identity.auth_source = AuthSource::PairingToken;

        let ctx = engine.build_context(identity);
        assert!(ctx.can_use_tool("file_read"));
        assert!(!ctx.can_use_tool("shell"));
    }

    #[test]
    fn from_nevis_identity() {
        let nevis = super::super::nevis::NevisIdentity {
            user_id: "nevis-user-123".into(),
            roles: vec!["operator".into()],
            scopes: vec!["openid".into()],
            mfa_verified: true,
            session_expiry: u64::MAX,
        };
        let identity = CallerIdentity::from_nevis(&nevis);
        assert_eq!(identity.user_id, "nevis-user-123");
        assert!(identity.has_role("operator"));
        assert!(identity.mfa_verified);
    }
}
