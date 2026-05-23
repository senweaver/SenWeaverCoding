// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::capabilities::{Capability, check_capabilities};
use super::iam_policy::{IamPolicy, PolicyDecision, RoleMapping};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerIdentity {

    pub user_id: String,

    pub display_name: Option<String>,

    pub roles: Vec<String>,

    pub auth_source: AuthSource,

    pub channel: Option<String>,

    pub mfa_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthSource {

    Cli,

    PairingToken,

    Nevis,

    Channel { platform: String },

    ApiKey,

    Anonymous,
}

impl CallerIdentity {

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

    pub fn has_role(&self, role: &str) -> bool {
        let normalized = role.trim().to_ascii_lowercase();
        self.roles
            .iter()
            .any(|r| r.trim().to_ascii_lowercase() == normalized)
    }

    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }
}

#[derive(Debug, Clone)]
pub struct AccessContext {

    pub identity: CallerIdentity,

    pub capabilities: Vec<Capability>,

    pub workspace: Option<String>,

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

    pub fn cli_operator() -> Self {
        let mut ctx = Self::new(CallerIdentity::cli_operator());
        ctx.capabilities = super::capabilities::default_capabilities();
        ctx
    }

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoleDefinition {

    pub name: String,

    pub description: String,

    pub allowed_tools: Vec<String>,

    #[serde(default)]
    pub allowed_workspaces: Vec<String>,

    #[serde(default)]
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UserRecord {

    pub user_id: String,

    #[serde(default)]
    pub display_name: String,

    pub roles: Vec<String>,

    #[serde(default = "default_true")]
    pub active: bool,

    #[serde(default)]
    pub created_at: u64,

    #[serde(default)]
    pub channel_bindings: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RbacConfig {

    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_role")]
    pub default_role: String,

    #[serde(default = "default_true")]
    pub cli_is_admin: bool,

    #[serde(default)]
    pub pairing_token_role: String,

    #[serde(default = "default_users_file")]
    pub users_file: String,

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

pub struct RbacEngine {
    config: RbacConfig,
    iam_policy: IamPolicy,
    roles: HashMap<String, RoleDefinition>,
    users: parking_lot::RwLock<HashMap<String, UserRecord>>,
    workspace_dir: PathBuf,
}

impl RbacEngine {

    pub fn allow_all() -> Self {
        let config = RbacConfig {
            enabled: false,
            ..RbacConfig::default()
        };

        Self::new(config, Path::new("."))
    }

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

        caps.push(Capability::LlmCall);
        caps.push(Capability::EnvRead);
        caps
    }

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
                        self.roles.entry(key).or_insert(role);
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
