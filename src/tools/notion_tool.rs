// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Tool, ToolResult};
use crate::security::{SecurityPolicy, policy::ToolOperation};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

const NOTION_API_BASE: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";
const NOTION_REQUEST_TIMEOUT_SECS: u64 = 30;

const MAX_ERROR_BODY_CHARS: usize = 500;

pub struct NotionTool {
    api_key: String,
    http: reqwest::Client,
    security: Arc<SecurityPolicy>,
}

impl NotionTool {

    pub fn new(api_key: String, security: Arc<SecurityPolicy>) -> Self {
        Self {
            api_key,
            http: reqwest::Client::new(),
            security,
        }
    }

    fn headers(&self) -> anyhow::Result<reqwest::header::HeaderMap> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.api_key)
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid Notion API key header value: {e}"))?,
        );
        headers.insert("Notion-Version", NOTION_VERSION.parse().unwrap());
        headers.insert("Content-Type", "application/json".parse().unwrap());
        Ok(headers)
    }

    async fn query_database(
        &self,
        database_id: &str,
        filter: Option<&serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{NOTION_API_BASE}/databases/{database_id}/query");
        let mut body = json!({});
        if let Some(f) = filter {
            body["filter"] = f.clone();
        }
        let resp = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(&body)
            .timeout(std::time::Duration::from_secs(NOTION_REQUEST_TIMEOUT_SECS))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let truncated = crate::util::truncate_with_ellipsis(&text, MAX_ERROR_BODY_CHARS);
            anyhow::bail!("Notion query_database failed ({status}): {truncated}");
        }
        resp.json().await.map_err(Into::into)
    }

    async fn read_page(&self, page_id: &str) -> anyhow::Result<serde_json::Value> {
        let url = format!("{NOTION_API_BASE}/pages/{page_id}");
        let resp = self
            .http
            .get(&url)
            .headers(self.headers()?)
            .timeout(std::time::Duration::from_secs(NOTION_REQUEST_TIMEOUT_SECS))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let truncated = crate::util::truncate_with_ellipsis(&text, MAX_ERROR_BODY_CHARS);
            anyhow::bail!("Notion read_page failed ({status}): {truncated}");
        }
        resp.json().await.map_err(Into::into)
    }

    async fn create_page(
        &self,
        properties: &serde_json::Value,
        database_id: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{NOTION_API_BASE}/pages");
        let mut body = json!({ "properties": properties });
        if let Some(db_id) = database_id {
            body["parent"] = json!({ "database_id": db_id });
        }
        let resp = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(&body)
            .timeout(std::time::Duration::from_secs(NOTION_REQUEST_TIMEOUT_SECS))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let truncated = crate::util::truncate_with_ellipsis(&text, MAX_ERROR_BODY_CHARS);
            anyhow::bail!("Notion create_page failed ({status}): {truncated}");
        }
        resp.json().await.map_err(Into::into)
    }

    async fn update_page(
        &self,
        page_id: &str,
        properties: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{NOTION_API_BASE}/pages/{page_id}");
        let body = json!({ "properties": properties });
        let resp = self
            .http
            .patch(&url)
            .headers(self.headers()?)
            .json(&body)
            .timeout(std::time::Duration::from_secs(NOTION_REQUEST_TIMEOUT_SECS))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let truncated = crate::util::truncate_with_ellipsis(&text, MAX_ERROR_BODY_CHARS);
            anyhow::bail!("Notion update_page failed ({status}): {truncated}");
        }
        resp.json().await.map_err(Into::into)
    }

    async fn search(&self, query: &str) -> anyhow::Result<serde_json::Value> {
        let url = format!("{NOTION_API_BASE}/search");
        let body = json!({ "query": query });
        let resp = self
            .http
            .post(&url)
            .headers(self.headers()?)
            .json(&body)
            .timeout(std::time::Duration::from_secs(NOTION_REQUEST_TIMEOUT_SECS))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let truncated = crate::util::truncate_with_ellipsis(&text, MAX_ERROR_BODY_CHARS);
            anyhow::bail!("Notion search failed ({status}): {truncated}");
        }
        resp.json().await.map_err(Into::into)
    }
}

#[async_trait]
impl Tool for NotionTool {
    fn name(&self) -> &str {
        "notion"
    }

    fn description(&self) -> &str {
        "Interact with Notion: query databases, read/create/update pages, and search the workspace."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["query_database", "read_page", "create_page", "update_page", "search"],
                    "description": "The Notion API action to perform"
                },
                "database_id": {
                    "type": "string",
                    "description": "Database ID (required for query_database, optional for create_page)"
                },
                "page_id": {
                    "type": "string",
                    "description": "Page ID (required for read_page and update_page)"
                },
                "filter": {
                    "type": "object",
                    "description": "Notion filter object for query_database"
                },
                "properties": {
                    "type": "object",
                    "description": "Properties object for create_page and update_page"
                },
                "query": {
                    "type": "string",
                    "description": "Search query string for the search action"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = match args.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("Missing required parameter: action".into()),
                });
            }
        };

        let operation = match action {
            "query_database" | "read_page" | "search" => ToolOperation::Read,
            "create_page" | "update_page" => ToolOperation::Act,
            _ => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Unknown action: {action}. Valid actions: query_database, read_page, create_page, update_page, search"
                    )),
                });
            }
        };

        if let Err(error) = self.security.enforce_tool_operation(operation, "notion") {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let result = match action {
            "query_database" => {
                let database_id = match args.get("database_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("query_database requires database_id parameter".into()),
                        });
                    }
                };
                let filter = args.get("filter");
                self.query_database(database_id, filter).await
            }
            "read_page" => {
                let page_id = match args.get("page_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("read_page requires page_id parameter".into()),
                        });
                    }
                };
                self.read_page(page_id).await
            }
            "create_page" => {
                let properties = match args.get("properties") {
                    Some(p) => p,
                    None => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("create_page requires properties parameter".into()),
                        });
                    }
                };
                let database_id = args.get("database_id").and_then(|v| v.as_str());
                self.create_page(properties, database_id).await
            }
            "update_page" => {
                let page_id = match args.get("page_id").and_then(|v| v.as_str()) {
                    Some(id) => id,
                    None => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("update_page requires page_id parameter".into()),
                        });
                    }
                };
                let properties = match args.get("properties") {
                    Some(p) => p,
                    None => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some("update_page requires properties parameter".into()),
                        });
                    }
                };
                self.update_page(page_id, properties).await
            }
            "search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                self.search(query).await
            }
            other => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "notion action '{other}' passed validation but has no dispatch arm (internal logic error)"
                    )),
                });
            }
        };

        match result {
            Ok(value) => Ok(ToolResult {
                success: true,
                output: serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}
