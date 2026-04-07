// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! A2A Protocol Client  - for making outbound agent-to-agent requests.
//!
//! This client handles discovering agents, sending tasks, and
//! checking task status on remote A2A-compliant agents.

use reqwest::Client;
use std::net::IpAddr;
use std::time::Duration;

use crate::gateway::a2a::types::{
    A2aTask, AgentCard, CancelTaskRequest, CancelTaskResponse, ListAgentsResponse, SendTaskRequest,
    SendTaskResponse, TaskId,
};

/// HTTP client for A2A protocol operations.
#[derive(Debug, Clone)]
pub struct A2aClient {
    http: Client,
    default_timeout: Duration,
}

impl A2aClient {
    /// Create a new A2A client with default settings.
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(30))
    }

    /// Create a new A2A client with a custom timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http,
            default_timeout: timeout,
        }
    }

    /// Discover an agent at a specific URL.
    ///
    /// Fetches the agent card from `/.well-known/agent.json`.
    ///
    /// SECURITY: Validates the target URL before making HTTP requests
    /// to prevent SSRF attacks targeting private/internal networks.
    pub async fn discover_agent(&self, url: &str) -> Result<AgentCard, A2aClientError> {
        // SSRF Protection: Validate URL before making outbound request.
        // Block private IPs, loopback, and link-local addresses.
        let parsed = reqwest::Url::parse(url).map_err(|e| A2aClientError::InvalidUrl {
            url: url.to_string(),
            message: format!("Failed to parse URL: {}", e),
        })?;

        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(A2aClientError::InvalidUrl {
                url: url.to_string(),
                message: "Only http/https schemes are allowed".to_string(),
            });
        }

        let host = parsed
            .host_str()
            .ok_or_else(|| A2aClientError::InvalidUrl {
                url: url.to_string(),
                message: "URL must have a host".to_string(),
            })?;

        fn is_private_v4(v4: std::net::Ipv4Addr) -> bool {
            let [a, b, _c, _d] = v4.octets();
            (a == 10) || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
        }

        fn is_private_ip(ip: IpAddr) -> bool {
            match ip {
                IpAddr::V4(v4) => is_private_v4(v4),
                IpAddr::V6(_) => false,
            }
        }

        fn is_link_local_ip(ip: IpAddr) -> bool {
            match ip {
                IpAddr::V4(v4) => v4.is_link_local(),
                IpAddr::V6(_) => false,
            }
        }

        // Check for private/localhost addresses
        if let Ok(ip) = host.parse::<IpAddr>() {
            if ip.is_loopback() || is_private_ip(ip) || is_link_local_ip(ip) || ip.is_unspecified()
            {
                return Err(A2aClientError::SsrfBlocked {
                    url: url.to_string(),
                    reason: "Connection to private/localhost addresses is not allowed".to_string(),
                });
            }
        }

        // Also check localhost in hostnames
        let host_lower = host.to_lowercase();
        if host_lower == "localhost"
            || host_lower.ends_with(".localhost")
            || host_lower.ends_with(".local")
            || host_lower.ends_with(".internal")
        {
            return Err(A2aClientError::SsrfBlocked {
                url: url.to_string(),
                reason: "Connection to localhost/internal hostnames is not allowed".to_string(),
            });
        }

        let well_known_url = format!("{}/.well-known/agent.json", url.trim_end_matches('/'));

        let response = self.http.get(&well_known_url).send().await.map_err(|e| {
            A2aClientError::RequestFailed {
                url: well_known_url.clone(),
                source: e,
            }
        })?;

        if !response.status().is_success() {
            return Err(A2aClientError::AgentNotFound {
                url: well_known_url,
                status: response.status().as_u16(),
            });
        }

        let agent_card: AgentCard =
            response
                .json()
                .await
                .map_err(|e| A2aClientError::InvalidResponse {
                    url: well_known_url,
                    message: format!("Failed to parse agent card: {}", e),
                })?;

        Ok(agent_card)
    }

    /// Send a task to an agent.
    ///
    /// POST /a2a/tasks/send on the agent's base URL.
    pub async fn send_task(
        &self,
        agent_url: &str,
        request: SendTaskRequest,
    ) -> Result<SendTaskResponse, A2aClientError> {
        let task_url = format!("{}/a2a/tasks/send", agent_url.trim_end_matches('/'));

        let response = self
            .http
            .post(&task_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| A2aClientError::RequestFailed {
                url: task_url.clone(),
                source: e,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(A2aClientError::TaskSendFailed {
                url: task_url,
                status: status.as_u16(),
                message: error_text,
            });
        }

        let task_response: SendTaskResponse =
            response
                .json()
                .await
                .map_err(|e| A2aClientError::InvalidResponse {
                    url: task_url,
                    message: format!("Failed to parse response: {}", e),
                })?;

        Ok(task_response)
    }

    /// Get task status from an agent.
    ///
    /// GET /a2a/tasks/{id} on the agent's base URL.
    pub async fn get_task(
        &self,
        agent_url: &str,
        task_id: &TaskId,
    ) -> Result<A2aTask, A2aClientError> {
        let task_url = format!("{}/a2a/tasks/{}", agent_url.trim_end_matches('/'), task_id);

        let response =
            self.http
                .get(&task_url)
                .send()
                .await
                .map_err(|e| A2aClientError::RequestFailed {
                    url: task_url.clone(),
                    source: e,
                })?;

        if response.status().as_u16() == 404 {
            return Err(A2aClientError::TaskNotFound {
                task_id: task_id.clone(),
                url: task_url,
            });
        }

        if !response.status().is_success() {
            return Err(A2aClientError::TaskQueryFailed {
                task_id: task_id.clone(),
                url: task_url,
                status: response.status().as_u16(),
            });
        }

        let task: A2aTask = response
            .json()
            .await
            .map_err(|e| A2aClientError::InvalidResponse {
                url: task_url,
                message: format!("Failed to parse task: {}", e),
            })?;

        Ok(task)
    }

    /// Cancel a task on an agent.
    ///
    /// POST /a2a/tasks/{id}/cancel on the agent's base URL.
    pub async fn cancel_task(
        &self,
        agent_url: &str,
        task_id: &TaskId,
        reason: Option<String>,
    ) -> Result<CancelTaskResponse, A2aClientError> {
        let cancel_url = format!(
            "{}/a2a/tasks/{}/cancel",
            agent_url.trim_end_matches('/'),
            task_id
        );

        let request = CancelTaskRequest { reason };

        let response = self
            .http
            .post(&cancel_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| A2aClientError::RequestFailed {
                url: cancel_url.clone(),
                source: e,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(A2aClientError::TaskCancelFailed {
                task_id: task_id.clone(),
                url: cancel_url,
                status: status.as_u16(),
                message: error_text,
            });
        }

        let cancel_response: CancelTaskResponse =
            response
                .json()
                .await
                .map_err(|e| A2aClientError::InvalidResponse {
                    url: cancel_url,
                    message: format!("Failed to parse response: {}", e),
                })?;

        Ok(cancel_response)
    }

    /// List agents from an A2A endpoint.
    ///
    /// GET /a2a/agents on the agent's base URL.
    pub async fn list_agents(&self, agent_url: &str) -> Result<ListAgentsResponse, A2aClientError> {
        let list_url = format!("{}/a2a/agents", agent_url.trim_end_matches('/'));

        let response =
            self.http
                .get(&list_url)
                .send()
                .await
                .map_err(|e| A2aClientError::RequestFailed {
                    url: list_url.clone(),
                    source: e,
                })?;

        if !response.status().is_success() {
            return Err(A2aClientError::AgentListFailed {
                url: list_url,
                status: response.status().as_u16(),
            });
        }

        let list_response: ListAgentsResponse =
            response
                .json()
                .await
                .map_err(|e| A2aClientError::InvalidResponse {
                    url: list_url,
                    message: format!("Failed to parse agent list: {}", e),
                })?;

        Ok(list_response)
    }

    /// Poll a task until it reaches a terminal state.
    ///
    /// This will repeatedly query the task status with the specified
    /// interval until the task is completed, failed, or cancelled.
    pub async fn poll_task_until_terminal(
        &self,
        agent_url: &str,
        task_id: &TaskId,
        poll_interval: Duration,
        max_polls: u32,
    ) -> Result<A2aTask, A2aClientError> {
        for i in 0..max_polls {
            let task = self.get_task(agent_url, task_id).await?;

            if task.is_terminal() {
                return Ok(task);
            }

            if i < max_polls - 1 {
                tokio::time::sleep(poll_interval).await;
            }
        }

        Err(A2aClientError::PollingTimeout {
            task_id: task_id.clone(),
            max_polls,
        })
    }
}

impl Default for A2aClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during A2A client operations.
#[derive(Debug, thiserror::Error)]
pub enum A2aClientError {
    #[error("Request failed to {url}: {source}")]
    RequestFailed { url: String, source: reqwest::Error },

    #[error("Agent not found at {url} (HTTP {status})")]
    AgentNotFound { url: String, status: u16 },

    #[error("Agent list failed at {url} (HTTP {status})")]
    AgentListFailed { url: String, status: u16 },

    #[error("Invalid URL {url}: {message}")]
    InvalidUrl { url: String, message: String },

    #[error("SSRF attack blocked: {url} — {reason}")]
    SsrfBlocked { url: String, reason: String },

    #[error("Invalid response from {url}: {message}")]
    InvalidResponse { url: String, message: String },

    #[error("Failed to send task to {url} (HTTP {status}): {message}")]
    TaskSendFailed {
        url: String,
        status: u16,
        message: String,
    },

    #[error("Task '{task_id}' not found at {url}")]
    TaskNotFound { task_id: TaskId, url: String },

    #[error("Failed to query task '{task_id}' at {url} (HTTP {status})")]
    TaskQueryFailed {
        task_id: TaskId,
        url: String,
        status: u16,
    },

    #[error("Failed to cancel task '{task_id}' at {url} (HTTP {status}): {message}")]
    TaskCancelFailed {
        task_id: TaskId,
        url: String,
        status: u16,
        message: String,
    },

    #[error("Polling timeout for task '{task_id}' after {max_polls} attempts")]
    PollingTimeout { task_id: TaskId, max_polls: u32 },
}

/// Utility to discover external agents from a list of URLs.
pub async fn discover_external_agents(
    client: &A2aClient,
    urls: &[String],
) -> Vec<(String, AgentCard)> {
    let mut discovered = Vec::new();

    for url in urls {
        match client.discover_agent(url).await {
            Ok(card) => {
                tracing::info!(agent_name = %card.name, url = %url, "discovered A2A agent");
                discovered.push((url.clone(), card));
            }
            Err(e) => {
                tracing::warn!(url = %url, error = %e, "failed to discover A2A agent");
            }
        }
    }

    discovered
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_discover_agent() {
        let mock_server = MockServer::start().await;

        let agent_card =
            AgentCard::new("Test Agent", "test-123", "A test agent", &mock_server.uri());

        Mock::given(method("GET"))
            .and(path("/.well-known/agent.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&agent_card))
            .mount(&mock_server)
            .await;

        let client = A2aClient::new();
        let discovered = client.discover_agent(&mock_server.uri()).await;

        assert!(discovered.is_ok());
        let card = discovered.unwrap();
        assert_eq!(card.name, "Test Agent");
    }

    #[tokio::test]
    async fn test_discover_agent_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/.well-known/agent.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let client = A2aClient::new();
        let result = client.discover_agent(&mock_server.uri()).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            A2aClientError::AgentNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_send_task() {
        let mock_server = MockServer::start().await;

        let task_response = SendTaskResponse {
            task: A2aTask::new("Test Task", "Do something"),
            estimated_completion_secs: Some(30),
        };

        Mock::given(method("POST"))
            .and(path("/a2a/tasks/send"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&task_response))
            .mount(&mock_server)
            .await;

        let client = A2aClient::new();
        let request = SendTaskRequest {
            name: "Test".to_string(),
            description: "Do something".to_string(),
            callback_url: None,
            metadata: Default::default(),
        };

        let result = client.send_task(&mock_server.uri(), request).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_default() {
        let client = A2aClient::default();
        assert_eq!(client.default_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_client_with_timeout() {
        let client = A2aClient::with_timeout(Duration::from_secs(60));
        assert_eq!(client.default_timeout, Duration::from_secs(60));
    }
}
