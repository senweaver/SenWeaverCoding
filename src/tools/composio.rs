// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use crate::security::policy::ToolOperation;
use anyhow::Context;
use async_trait::async_trait;
use parking_lot::RwLock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

const COMPOSIO_API_BASE_V3: &str = "https://backend.composio.dev/api/v3";
const COMPOSIO_TOOL_VERSION_LATEST: &str = "latest";

fn ensure_https(url: &str) -> anyhow::Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!(
            "Refusing to transmit sensitive data over non-HTTPS URL: URL scheme must be https"
        );
    }
    Ok(())
}

pub struct ComposioTool {
    api_key: String,
    default_entity_id: String,
    security: Arc<SecurityPolicy>,
    recent_connected_accounts: RwLock<HashMap<String, String>>,
    action_slug_cache: RwLock<HashMap<String, String>>,
}

impl ComposioTool {
    pub fn new(
        api_key: &str,
        default_entity_id: Option<&str>,
        security: Arc<SecurityPolicy>,
    ) -> Self {
        Self {
            api_key: api_key.to_string(),
            default_entity_id: normalize_entity_id(default_entity_id.unwrap_or("default")),
            security,
            recent_connected_accounts: RwLock::new(HashMap::new()),
            action_slug_cache: RwLock::new(HashMap::new()),
        }
    }

    fn client(&self) -> Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client_with_timeouts("tool.composio", 60, 10)
    }

    pub async fn list_actions(
        &self,
        app_name: Option<&str>,
    ) -> anyhow::Result<Vec<ComposioAction>> {
        self.list_actions_v3(app_name).await
    }

    async fn list_actions_v3(&self, app_name: Option<&str>) -> anyhow::Result<Vec<ComposioAction>> {
        let url = format!("{COMPOSIO_API_BASE_V3}/tools");
        let req = self
            .client()
            .get(&url)
            .header("x-api-key", &self.api_key)
            .query(&Self::build_list_actions_v3_query(app_name));

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 API error: {err}");
        }

        let body: ComposioToolsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v3 tools response")?;
        self.update_action_slug_cache_from_v3_items(&body.items);
        Ok(map_v3_tools_to_actions(body.items))
    }

    fn update_action_slug_cache_from_v3_items(&self, items: &[ComposioV3Tool]) {
        for item in items {
            let Some(slug) = item.slug.as_deref().or(item.name.as_deref()) else {
                continue;
            };
            self.cache_action_slug(slug, slug);
            if let Some(name) = item.name.as_deref() {
                self.cache_action_slug(name, slug);
            }
        }
    }

    async fn list_connected_accounts(
        &self,
        app_name: Option<&str>,
        entity_id: Option<&str>,
    ) -> anyhow::Result<Vec<ComposioConnectedAccount>> {
        let url = format!("{COMPOSIO_API_BASE_V3}/connected_accounts");
        let mut req = self.client().get(&url).header("x-api-key", &self.api_key);

        req = req.query(&[
            ("limit", "50"),
            ("order_by", "updated_at"),
            ("order_direction", "desc"),
            ("statuses", "INITIALIZING"),
            ("statuses", "ACTIVE"),
            ("statuses", "INITIATED"),
        ]);

        if let Some(app) = app_name
            .map(normalize_app_slug)
            .filter(|app| !app.is_empty())
        {
            req = req.query(&[("toolkit_slugs", app.as_str())]);
        }

        if let Some(entity) = entity_id {
            req = req.query(&[("user_ids", entity)]);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 connected accounts lookup failed: {err}");
        }

        let body: ComposioConnectedAccountsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v3 connected accounts response")?;
        Ok(body.items)
    }

    fn cache_connected_account(&self, app_name: &str, entity_id: &str, connected_account_id: &str) {
        let key = connected_account_cache_key(app_name, entity_id);
        self.recent_connected_accounts
            .write()
            .insert(key, connected_account_id.to_string());
    }

    fn get_cached_connected_account(&self, app_name: &str, entity_id: &str) -> Option<String> {
        let key = connected_account_cache_key(app_name, entity_id);
        self.recent_connected_accounts.read().get(&key).cloned()
    }

    async fn resolve_connected_account_ref(
        &self,
        app_name: Option<&str>,
        entity_id: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let app = app_name
            .map(normalize_app_slug)
            .filter(|app| !app.is_empty());
        let entity = entity_id.map(normalize_entity_id);
        let (Some(app), Some(entity)) = (app, entity) else {
            return Ok(None);
        };

        if let Some(cached) = self.get_cached_connected_account(&app, &entity) {
            return Ok(Some(cached));
        }

        let accounts = self
            .list_connected_accounts(Some(&app), Some(&entity))
            .await?;

        let Some(first) = accounts.into_iter().find(|acct| acct.is_usable()) else {
            return Ok(None);
        };

        self.cache_connected_account(&app, &entity, &first.id);
        Ok(Some(first.id))
    }

    pub async fn execute_action(
        &self,
        action_name: &str,
        app_name_hint: Option<&str>,
        params: serde_json::Value,
        text: Option<&str>,
        entity_id: Option<&str>,
        connected_account_ref: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let app_hint = app_name_hint
            .map(normalize_app_slug)
            .filter(|app| !app.is_empty())
            .or_else(|| infer_app_slug_from_action_name(action_name));
        let normalized_entity_id = entity_id.map(normalize_entity_id);
        let explicit_account_ref = connected_account_ref.and_then(|candidate| {
            let trimmed = candidate.trim();
            (!trimmed.is_empty()).then_some(trimmed.to_string())
        });
        let resolved_account_ref = if explicit_account_ref.is_some() {
            explicit_account_ref
        } else {
            self.resolve_connected_account_ref(app_hint.as_deref(), normalized_entity_id.as_deref())
                .await?
        };

        let mut slug_candidates = self.build_v3_slug_candidates(action_name);
        let mut prime_error = None;
        if slug_candidates.is_empty() {
            if let Some(app) = app_hint.as_deref() {
                match self.list_actions(Some(app)).await {
                    Ok(_) => {
                        slug_candidates = self.build_v3_slug_candidates(action_name);
                    }
                    Err(err) => {
                        prime_error = Some(format!(
                            "Failed to refresh action list for app '{app}': {err}"
                        ));
                    }
                }
            }
        }

        if slug_candidates.is_empty() {
            anyhow::bail!(
                "Unable to determine tool slug for '{action_name}'. Run action='list' with the relevant app first to prime the cache.{}",
                prime_error
                    .as_deref()
                    .map(|msg| format!(" ({msg})"))
                    .unwrap_or_default()
            );
        }

        let mut v3_errors = Vec::new();
        for slug in slug_candidates {
            self.cache_action_slug(action_name, &slug);
            match self
                .execute_action_v3(
                    &slug,
                    params.clone(),
                    text,
                    normalized_entity_id.as_deref(),
                    resolved_account_ref.as_deref(),
                )
                .await
            {
                Ok(result) => return Ok(result),
                Err(err) => v3_errors.push(format!("{slug}: {err}")),
            }
        }

        let v3_error_summary = if v3_errors.is_empty() {
            "no v3 candidates attempted".to_string()
        } else {
            v3_errors.join(" | ")
        };

        let prime_suffix = prime_error
            .as_deref()
            .map(|msg| format!(" ({msg})"))
            .unwrap_or_default();

        if text.is_some() {
            anyhow::bail!(
                "Composio v3 NLP execute failed on candidates ({v3_error_summary}){prime_suffix}{}",
                build_connected_account_hint(
                    app_hint.as_deref(),
                    normalized_entity_id.as_deref(),
                    resolved_account_ref.as_deref(),
                )
            );
        }

        anyhow::bail!(
            "Composio execute failed on v3 ({v3_error_summary}){prime_suffix}{}",
            build_connected_account_hint(
                app_hint.as_deref(),
                normalized_entity_id.as_deref(),
                resolved_account_ref.as_deref(),
            )
        );
    }

    fn build_v3_slug_candidates(&self, action_name: &str) -> Vec<String> {
        let mut candidates = Vec::new();
        let mut push_candidate = |candidate: String| {
            if !candidate.is_empty() && !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        };

        if let Some(hit) = self.lookup_cached_action_slug(action_name) {
            push_candidate(hit);
        }

        for slug in build_tool_slug_candidates(action_name) {
            push_candidate(slug);
        }

        candidates
    }

    fn cache_action_slug(&self, alias: &str, slug: &str) {
        let Some(key) = normalize_action_cache_key(alias) else {
            return;
        };
        let trimmed_slug = slug.trim();
        if trimmed_slug.is_empty() {
            return;
        }
        self.action_slug_cache
            .write()
            .insert(key, trimmed_slug.to_string());
    }

    fn lookup_cached_action_slug(&self, action_name: &str) -> Option<String> {
        let key = normalize_action_cache_key(action_name)?;
        self.action_slug_cache.read().get(&key).cloned()
    }

    fn build_list_actions_v3_query(app_name: Option<&str>) -> Vec<(String, String)> {
        let mut query = vec![
            ("limit".to_string(), "200".to_string()),
            (
                "toolkit_versions".to_string(),
                COMPOSIO_TOOL_VERSION_LATEST.to_string(),
            ),
        ];

        if let Some(app) = app_name.map(str::trim).filter(|app| !app.is_empty()) {
            query.push(("toolkits".to_string(), app.to_string()));
            query.push(("toolkit_slug".to_string(), app.to_string()));
        }

        query
    }

    fn build_execute_action_v3_request(
        tool_slug: &str,
        params: serde_json::Value,
        text: Option<&str>,
        entity_id: Option<&str>,
        connected_account_ref: Option<&str>,
    ) -> (String, serde_json::Value) {
        let url = format!("{COMPOSIO_API_BASE_V3}/tools/execute/{tool_slug}");
        let account_ref = connected_account_ref.and_then(|candidate| {
            let trimmed_candidate = candidate.trim();
            (!trimmed_candidate.is_empty()).then_some(trimmed_candidate)
        });

        let mut body = json!({
            "version": COMPOSIO_TOOL_VERSION_LATEST,
        });

        if let Some(nl_text) = text {
            body["text"] = json!(nl_text);
        } else {
            body["arguments"] = params;
        }

        if let Some(entity) = entity_id {
            body["user_id"] = json!(entity);
        }
        if let Some(account_ref) = account_ref {
            body["connected_account_id"] = json!(account_ref);
        }

        (url, body)
    }

    async fn execute_action_v3(
        &self,
        tool_slug: &str,
        params: serde_json::Value,
        text: Option<&str>,
        entity_id: Option<&str>,
        connected_account_ref: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let (url, body) = Self::build_execute_action_v3_request(
            tool_slug,
            params,
            text,
            entity_id,
            connected_account_ref,
        );

        ensure_https(&url)?;

        let resp = self
            .client()
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 action execution failed: {err}");
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .context("Failed to decode Composio v3 execute response")?;
        Ok(result)
    }

    pub async fn get_connection_url(
        &self,
        app_name: Option<&str>,
        auth_config_id: Option<&str>,
        entity_id: &str,
    ) -> anyhow::Result<ComposioConnectionLink> {
        self.get_connection_url_v3(app_name, auth_config_id, entity_id)
            .await
    }

    async fn get_connection_url_v3(
        &self,
        app_name: Option<&str>,
        auth_config_id: Option<&str>,
        entity_id: &str,
    ) -> anyhow::Result<ComposioConnectionLink> {
        let auth_config_id = match auth_config_id {
            Some(id) => id.to_string(),
            None => {
                let app = app_name.ok_or_else(|| {
                    anyhow::anyhow!("Missing 'app' or 'auth_config_id' for v3 connect")
                })?;
                self.resolve_auth_config_id(app).await?
            }
        };

        let url = format!("{COMPOSIO_API_BASE_V3}/connected_accounts/link");
        let body = json!({
            "auth_config_id": auth_config_id,
            "user_id": entity_id,
        });

        let resp = self
            .client()
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 connect failed: {err}");
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .context("Failed to decode Composio v3 connect response")?;
        let redirect_url = extract_redirect_url(&result)
            .ok_or_else(|| anyhow::anyhow!("No redirect URL in Composio v3 response"))?;
        Ok(ComposioConnectionLink {
            redirect_url,
            connected_account_id: extract_connected_account_id(&result),
        })
    }

    async fn get_tool_schema(&self, tool_slug: &str) -> anyhow::Result<serde_json::Value> {
        let slug = normalize_tool_slug(tool_slug);
        let url = format!("{COMPOSIO_API_BASE_V3}/tools/{slug}");
        ensure_https(&url)?;

        let resp = self
            .client()
            .get(&url)
            .header("x-api-key", &self.api_key)
            .query(&[("version", COMPOSIO_TOOL_VERSION_LATEST)])
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 tool schema lookup failed for '{slug}': {err}");
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to decode Composio v3 tool schema response")?;
        Ok(body)
    }

    async fn resolve_auth_config_id(&self, app_name: &str) -> anyhow::Result<String> {
        let url = format!("{COMPOSIO_API_BASE_V3}/auth_configs");

        let resp = self
            .client()
            .get(&url)
            .header("x-api-key", &self.api_key)
            .query(&[
                ("toolkit_slug", app_name),
                ("show_disabled", "true"),
                ("limit", "25"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 auth config lookup failed: {err}");
        }

        let body: ComposioAuthConfigsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v3 auth configs response")?;

        if body.items.is_empty() {
            anyhow::bail!(
                "No auth config found for toolkit '{app_name}'. Create one in Composio first."
            );
        }

        let preferred = body
            .items
            .iter()
            .find(|cfg| cfg.is_enabled())
            .or_else(|| body.items.first())
            .context("No usable auth config returned by Composio")?;

        Ok(preferred.id.clone())
    }
}

#[async_trait]
impl Tool for ComposioTool {
    fn name(&self) -> &str {
        "composio"
    }

    fn description(&self) -> &str {
        "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). \
         Use action='list' to see available actions (includes parameter names). \
         action='execute' with action_name/tool_slug and params to run an action. \
         If you are unsure of the exact params, pass 'text' instead with a natural-language description \
         of what you want (Composio will resolve the correct parameters via NLP). \
         action='list_accounts' or action='connected_accounts' to list OAuth-connected accounts. \
         action='connect' with app/auth_config_id to get OAuth URL. \
         connected_account_id is auto-resolved when omitted."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "The operation: 'list' (list available actions), 'list_accounts'/'connected_accounts' (list connected accounts), 'execute' (run an action), or 'connect' (get OAuth URL)",
                    "enum": ["list", "list_accounts", "connected_accounts", "execute", "connect"]
                },
                "app": {
                    "type": "string",
                    "description": "Toolkit slug filter for 'list' or 'list_accounts', optional app hint for 'execute', or toolkit/app for 'connect' (e.g. 'gmail', 'notion', 'github')"
                },
                "action_name": {
                    "type": "string",
                    "description": "Action/tool identifier to execute (legacy aliases supported)"
                },
                "tool_slug": {
                    "type": "string",
                    "description": "Preferred v3 tool slug to execute (alias of action_name)"
                },
                "params": {
                    "type": "object",
                    "description": "Structured parameters to pass to the action (use the key names shown by action='list')"
                },
                "text": {
                    "type": "string",
                    "description": "Natural-language description of what you want the action to do (alternative to 'params' when you are unsure of the exact parameter names). Composio will resolve the correct parameters via NLP. Mutually exclusive with 'params'."
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity/user ID for multi-user setups (defaults to composio.entity_id from config)"
                },
                "auth_config_id": {
                    "type": "string",
                    "description": "Optional Composio v3 auth config id for connect flow"
                },
                "connected_account_id": {
                    "type": "string",
                    "description": "Optional connected account ID for execute flow when a specific account is required"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        let entity_id = args
            .get("entity_id")
            .and_then(|v| v.as_str())
            .unwrap_or(self.default_entity_id.as_str());

        match action {
            "list" => {
                let app = args.get("app").and_then(|v| v.as_str());
                match self.list_actions(app).await {
                    Ok(actions) => {
                        let summary: Vec<String> = actions
                            .iter()
                            .take(20)
                            .map(|a| {
                                let params_hint =
                                    format_input_params_hint(a.input_parameters.as_ref());
                                format!(
                                    "- {} ({}): {}{}",
                                    a.name,
                                    a.app_name.as_deref().unwrap_or("?"),
                                    a.description.as_deref().unwrap_or(""),
                                    params_hint,
                                )
                            })
                            .collect();
                        let total = actions.len();
                        let output = format!(
                            "Found {total} available actions:\n{}{}",
                            summary.join("\n"),
                            if total > 20 {
                                format!("\n... and {} more", total - 20)
                            } else {
                                String::new()
                            }
                        );
                        Ok(ToolResult {
                            success: true,
                            output,
                            error: None,
                        })
                    }
                    Err(e) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to list actions: {e}")),
                    }),
                }
            }

            "list_accounts" | "connected_accounts" => {
                let app = args.get("app").and_then(|v| v.as_str());
                match self.list_connected_accounts(app, Some(entity_id)).await {
                    Ok(accounts) => {
                        if accounts.is_empty() {
                            let app_hint = app
                                .map(|value| format!(" for app '{value}'"))
                                .unwrap_or_default();
                            return Ok(ToolResult {
                                success: true,
                                output: format!(
                                    "No connected accounts found{app_hint} for entity '{entity_id}'. Run action='connect' first."
                                ),
                                error: None,
                            });
                        }

                        let summary: Vec<String> = accounts
                            .iter()
                            .take(20)
                            .map(|account| {
                                let toolkit = account.toolkit_slug().unwrap_or("?");
                                format!("- {} [{}] toolkit={toolkit}", account.id, account.status)
                            })
                            .collect();
                        let total = accounts.len();
                        let output = format!(
                            "Found {total} connected accounts (entity '{entity_id}'):\n{}{}\nUse connected_account_id in action='execute' when needed.",
                            summary.join("\n"),
                            if total > 20 {
                                format!("\n... and {} more", total - 20)
                            } else {
                                String::new()
                            }
                        );
                        Ok(ToolResult {
                            success: true,
                            output,
                            error: None,
                        })
                    }
                    Err(e) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to list connected accounts: {e}")),
                    }),
                }
            }

            "execute" => {
                if let Err(error) = self
                    .security
                    .enforce_tool_operation(ToolOperation::Act, "composio.execute")
                {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(error),
                    });
                }

                let action_name = args
                    .get("tool_slug")
                    .or_else(|| args.get("action_name"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Missing 'action_name' (or 'tool_slug') for execute")
                    })?;

                let app = args.get("app").and_then(|v| v.as_str());
                let params = args.get("params").cloned().unwrap_or(json!({}));
                let text = args.get("text").and_then(|v| v.as_str());
                let acct_ref = args.get("connected_account_id").and_then(|v| v.as_str());

                match self
                    .execute_action(action_name, app, params, text, Some(entity_id), acct_ref)
                    .await
                {
                    Ok(result) => {
                        let output = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| format!("{result:?}"));
                        Ok(ToolResult {
                            success: true,
                            output,
                            error: None,
                        })
                    }
                    Err(e) => {

                        let schema_hint = self
                            .get_tool_schema(action_name)
                            .await
                            .ok()
                            .and_then(|s| format_schema_hint(&s))
                            .unwrap_or_default();
                        Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Action execution failed: {e}{schema_hint}")),
                        })
                    }
                }
            }

            "connect" => {
                if let Err(error) = self
                    .security
                    .enforce_tool_operation(ToolOperation::Act, "composio.connect")
                {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(error),
                    });
                }

                let app = args.get("app").and_then(|v| v.as_str());
                let auth_config_id = args.get("auth_config_id").and_then(|v| v.as_str());

                if app.is_none() && auth_config_id.is_none() {
                    anyhow::bail!("Missing 'app' or 'auth_config_id' for connect");
                }

                match self
                    .get_connection_url(app, auth_config_id, entity_id)
                    .await
                {
                    Ok(link) => {
                        let target =
                            app.unwrap_or(auth_config_id.unwrap_or("provided auth config"));
                        let mut output =
                            format!("Open this URL to connect {target}:\n{}", link.redirect_url);
                        if let Some(connected_account_id) = link.connected_account_id.as_deref() {
                            if let Some(app_name) = app {
                                self.cache_connected_account(
                                    app_name,
                                    entity_id,
                                    connected_account_id,
                                );
                            }
                            let _ =
                                write!(output, "\nConnected account ID: {connected_account_id}");
                        }
                        Ok(ToolResult {
                            success: true,
                            output,
                            error: None,
                        })
                    }
                    Err(e) => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!("Failed to get connection URL: {e}")),
                    }),
                }
            }

            _ => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action '{action}'. Use 'list', 'list_accounts', 'execute', or 'connect'."
                )),
            }),
        }
    }
}

fn normalize_entity_id(entity_id: &str) -> String {
    let trimmed = entity_id.trim();
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_tool_slug(action_name: &str) -> String {
    action_name.trim().replace('_', "-").to_ascii_lowercase()
}

fn build_tool_slug_candidates(action_name: &str) -> Vec<String> {
    let trimmed = action_name.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push_candidate = |candidate: String| {
        if !candidate.is_empty() && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    };

    push_candidate(trimmed.to_string());
    push_candidate(normalize_tool_slug(trimmed));

    let lower = trimmed.to_ascii_lowercase();
    push_candidate(lower.clone());

    let underscore_lower = lower.replace('-', "_");
    push_candidate(underscore_lower);

    let hyphen_lower = lower.replace('_', "-");
    push_candidate(hyphen_lower);

    let upper = trimmed.to_ascii_uppercase();
    push_candidate(upper.clone());
    push_candidate(upper.replace('-', "_"));
    push_candidate(upper.replace('_', "-"));

    candidates
}

fn normalize_app_slug(app_name: &str) -> String {
    app_name
        .trim()
        .replace('_', "-")
        .to_ascii_lowercase()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn infer_app_slug_from_action_name(action_name: &str) -> Option<String> {
    let trimmed = action_name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let raw = if trimmed.contains('-') {
        trimmed.split('-').next()
    } else if trimmed.contains('_') {
        trimmed.split('_').next()
    } else {
        None
    }?;

    let app = normalize_app_slug(raw);
    (!app.is_empty()).then_some(app)
}

fn connected_account_cache_key(app_name: &str, entity_id: &str) -> String {
    format!(
        "{}:{}",
        normalize_entity_id(entity_id),
        normalize_app_slug(app_name)
    )
}

fn normalize_action_cache_key(alias: &str) -> Option<String> {
    let trimmed = alias.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(
        trimmed
            .to_ascii_lowercase()
            .replace('_', "-")
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-"),
    )
}

fn build_connected_account_hint(
    app_hint: Option<&str>,
    entity_id: Option<&str>,
    connected_account_ref: Option<&str>,
) -> String {
    if connected_account_ref.is_some() {
        return String::new();
    }

    let Some(entity) = entity_id else {
        return String::new();
    };

    if let Some(app) = app_hint {
        format!(
            " Hint: use action='list_accounts' with app='{app}' and entity_id='{entity}' to retrieve connected_account_id."
        )
    } else {
        format!(
            " Hint: use action='list_accounts' with entity_id='{entity}' to retrieve connected_account_id."
        )
    }
}

fn map_v3_tools_to_actions(items: Vec<ComposioV3Tool>) -> Vec<ComposioAction> {
    items
        .into_iter()
        .filter_map(|item| {
            let name = item.slug.or(item.name.clone())?;
            let app_name = item
                .toolkit
                .as_ref()
                .and_then(|toolkit| toolkit.slug.clone().or(toolkit.name.clone()))
                .or(item.app_name);
            let description = item.description.or(item.name);
            Some(ComposioAction {
                name,
                app_name,
                description,
                enabled: true,
                input_parameters: item.input_parameters,
            })
        })
        .collect()
}

fn extract_redirect_url(result: &serde_json::Value) -> Option<String> {
    result
        .get("redirect_url")
        .and_then(|v| v.as_str())
        .or_else(|| result.get("redirectUrl").and_then(|v| v.as_str()))
        .or_else(|| {
            result
                .get("data")
                .and_then(|v| v.get("redirect_url"))
                .and_then(|v| v.as_str())
        })
        .map(ToString::to_string)
}

fn extract_connected_account_id(result: &serde_json::Value) -> Option<String> {
    result
        .get("connected_account_id")
        .and_then(|v| v.as_str())
        .or_else(|| result.get("connectedAccountId").and_then(|v| v.as_str()))
        .or_else(|| {
            result
                .get("data")
                .and_then(|v| v.get("connected_account_id"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            result
                .get("data")
                .and_then(|v| v.get("connectedAccountId"))
                .and_then(|v| v.as_str())
        })
        .map(ToString::to_string)
}

async fn response_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if body.trim().is_empty() {
        return format!("HTTP {}", status.as_u16());
    }

    if let Some(api_error) = extract_api_error_message(&body) {
        return format!(
            "HTTP {}: {}",
            status.as_u16(),
            sanitize_error_message(&api_error)
        );
    }

    format!("HTTP {}", status.as_u16())
}

fn sanitize_error_message(message: &str) -> String {
    let mut sanitized = message.replace('\n', " ");
    for marker in [
        "connected_account_id",
        "connectedAccountId",
        "entity_id",
        "entityId",
        "user_id",
        "userId",
    ] {
        sanitized = sanitized.replace(marker, "[redacted]");
    }

    let max_chars = 240;
    if sanitized.chars().count() <= max_chars {
        sanitized
    } else {
        let mut end = max_chars;
        while end > 0 && !sanitized.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &sanitized[..end])
    }
}

fn extract_api_error_message(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed
        .get("error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            parsed
                .get("message")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
}

fn format_input_params_hint(schema: Option<&serde_json::Value>) -> String {
    let props = schema
        .and_then(|v| v.get("properties"))
        .and_then(|v| v.as_object());
    let required: Vec<&str> = schema
        .and_then(|v| v.get("required"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let Some(props) = props else {
        return String::new();
    };
    if props.is_empty() {
        return String::new();
    }

    let keys: Vec<String> = props
        .keys()
        .map(|k| {
            if required.contains(&k.as_str()) {
                format!("{k}*")
            } else {
                k.clone()
            }
        })
        .collect();
    format!(" [params: {}]", keys.join(", "))
}

fn floor_char_boundary_compat(text: &str, index: usize) -> usize {
    let mut end = index.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn format_schema_hint(schema: &serde_json::Value) -> Option<String> {
    let input_params = schema.get("input_parameters")?;
    let props = input_params.get("properties")?.as_object()?;
    if props.is_empty() {
        return None;
    }

    let required: Vec<&str> = input_params
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut lines = Vec::new();
    for (key, spec) in props {
        let type_str = spec.get("type").and_then(|v| v.as_str()).unwrap_or("any");
        let desc = spec
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let req = if required.contains(&key.as_str()) {
            " (required)"
        } else {
            ""
        };
        let desc_suffix = if desc.is_empty() {
            String::new()
        } else {

            let short = if desc.len() > 80 {
                let end = floor_char_boundary_compat(desc, 77);
                format!("{}...", &desc[..end])
            } else {
                desc.to_string()
            };
            format!(" - {short}")
        };
        lines.push(format!("  {key}: {type_str}{req}{desc_suffix}"));
    }

    Some(format!(
        "\n\nExpected input parameters:\n{}",
        lines.join("\n")
    ))
}

#[derive(Debug, Deserialize)]
struct ComposioToolsResponse {
    #[serde(default)]
    items: Vec<ComposioV3Tool>,
}

#[derive(Debug, Deserialize)]
struct ComposioConnectedAccountsResponse {
    #[serde(default)]
    items: Vec<ComposioConnectedAccount>,
}

#[derive(Debug, Clone, Deserialize)]
struct ComposioConnectedAccount {
    id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    toolkit: Option<ComposioToolkitRef>,
}

impl ComposioConnectedAccount {
    fn is_usable(&self) -> bool {
        self.status.eq_ignore_ascii_case("INITIALIZING")
            || self.status.eq_ignore_ascii_case("ACTIVE")
            || self.status.eq_ignore_ascii_case("INITIATED")
    }

    fn toolkit_slug(&self) -> Option<&str> {
        self.toolkit
            .as_ref()
            .and_then(|toolkit| toolkit.slug.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ComposioV3Tool {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "appName", default)]
    app_name: Option<String>,
    #[serde(default)]
    toolkit: Option<ComposioToolkitRef>,

    #[serde(default)]
    input_parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct ComposioToolkitRef {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ComposioAuthConfigsResponse {
    #[serde(default)]
    items: Vec<ComposioAuthConfig>,
}

#[derive(Debug, Clone)]
pub struct ComposioConnectionLink {
    pub redirect_url: String,
    pub connected_account_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ComposioAuthConfig {
    id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

impl ComposioAuthConfig {
    fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
            || self
                .status
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case("enabled"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioAction {
    pub name: String,
    #[serde(rename = "appName")]
    pub app_name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_parameters: Option<serde_json::Value>,
}
