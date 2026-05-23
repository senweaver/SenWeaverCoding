// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub description: String,
    pub input_summary: String,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub request_id: String,
    pub approved: bool,
    pub reason: Option<String>,
}

pub struct RemotePermissionBridge {
    pending: std::collections::HashMap<String, oneshot::Sender<PermissionResponse>>,
}

impl RemotePermissionBridge {
    pub fn new() -> Self {
        Self {
            pending: std::collections::HashMap::new(),
        }
    }

    pub async fn request_permission(
        &mut self,
        tool_name: &str,
        description: &str,
        input_summary: &str,
        risk_level: RiskLevel,
    ) -> anyhow::Result<PermissionResponse> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let _request = PermissionRequest {
            request_id: request_id.clone(),
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            input_summary: input_summary.to_string(),
            risk_level,
        };

        self.pending.insert(request_id.clone(), tx);

        let response = rx
            .await
            .map_err(|_| anyhow::anyhow!("Permission request cancelled"))?;
        Ok(response)
    }

    pub fn resolve_permission(&mut self, response: PermissionResponse) -> bool {
        if let Some(tx) = self.pending.remove(&response.request_id) {
            tx.send(response).is_ok()
        } else {
            false
        }
    }

    pub fn cancel_all(&mut self) {
        self.pending.clear();
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

impl Default for RemotePermissionBridge {
    fn default() -> Self {
        Self::new()
    }
}
