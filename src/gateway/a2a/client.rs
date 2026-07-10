// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use reqwest::Client;
use std::net::IpAddr;
use std::time::Duration;

use crate::gateway::a2a::types::{
    A2aTask, AgentCard, CancelTaskRequest, CancelTaskResponse, ListAgentsResponse, SendTaskRequest,
    SendTaskResponse, TaskId,
};

fn ipv4_is_blocked(v4: std::net::Ipv4Addr) -> bool {
    let [a, b, _c, _d] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || (a == 100 && (64..=127).contains(&b))
        || a == 0
}

fn ip_is_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_blocked(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ipv4_is_blocked(v4);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct A2aClient {
    http: Client,
    default_timeout: Duration,
}

impl A2aClient {

    pub fn new() -> Result<Self, A2aClientError> {
        Self::with_timeout(Duration::from_secs(30))
    }

    pub fn with_timeout(timeout: Duration) -> Result<Self, A2aClientError> {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| A2aClientError::ClientBuild { source: e })?;

        Ok(Self {
            http,
            default_timeout: timeout,
        })
    }

    async fn validate_agent_url(&self, url: &str) -> Result<(), A2aClientError> {
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

        let host_trimmed = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = host_trimmed.parse::<IpAddr>() {
            if ip_is_blocked(ip) {
                return Err(A2aClientError::SsrfBlocked {
                    url: url.to_string(),
                    reason: "Connection to private/localhost addresses is not allowed".to_string(),
                });
            }
            return Ok(());
        }

        let port = parsed.port_or_known_default().unwrap_or(443);
        match tokio::net::lookup_host((host_trimmed, port)).await {
            Ok(addrs) => {
                for addr in addrs {
                    if ip_is_blocked(addr.ip()) {
                        return Err(A2aClientError::SsrfBlocked {
                            url: url.to_string(),
                            reason: format!(
                                "Hostname resolves to blocked address {} (possible DNS \
                                 rebinding)",
                                addr.ip()
                            ),
                        });
                    }
                }
            }
            Err(e) => {
                return Err(A2aClientError::InvalidUrl {
                    url: url.to_string(),
                    message: format!("Failed to resolve host: {}", e),
                });
            }
        }

        Ok(())
    }

    pub async fn discover_agent(&self, url: &str) -> Result<AgentCard, A2aClientError> {
        self.validate_agent_url(url).await?;

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

    pub async fn send_task(
        &self,
        agent_url: &str,
        request: SendTaskRequest,
    ) -> Result<SendTaskResponse, A2aClientError> {
        self.validate_agent_url(agent_url).await?;
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

    pub async fn get_task(
        &self,
        agent_url: &str,
        task_id: &TaskId,
    ) -> Result<A2aTask, A2aClientError> {
        self.validate_agent_url(agent_url).await?;
        let task_url = format!("{}/a2a/tasks/{}", agent_url.trim_end_matches('/'), task_id);

        let policy = crate::util::retry::RetryPolicy::http();
        let response = crate::util::retry::retry(&policy, |_attempt| {
            let url = task_url.clone();
            let http = self.http.clone();
            async move {
                http.get(&url)
                    .send()
                    .await
                    .map_err(|e| A2aClientError::RequestFailed { url, source: e })
            }
        })
        .await?;

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

    pub async fn cancel_task(
        &self,
        agent_url: &str,
        task_id: &TaskId,
        reason: Option<String>,
    ) -> Result<CancelTaskResponse, A2aClientError> {
        self.validate_agent_url(agent_url).await?;
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

    pub async fn list_agents(&self, agent_url: &str) -> Result<ListAgentsResponse, A2aClientError> {
        self.validate_agent_url(agent_url).await?;
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

#[derive(Debug, thiserror::Error)]
pub enum A2aClientError {
    #[error("Failed to build A2A HTTP client: {source}")]
    ClientBuild { source: reqwest::Error },

    #[error("Request failed to {url}: {source}")]
    RequestFailed { url: String, source: reqwest::Error },

    #[error("Agent not found at {url} (HTTP {status})")]
    AgentNotFound { url: String, status: u16 },

    #[error("Agent list failed at {url} (HTTP {status})")]
    AgentListFailed { url: String, status: u16 },

    #[error("Invalid URL {url}: {message}")]
    InvalidUrl { url: String, message: String },

    #[error("SSRF attack blocked: {url}  -  {reason}")]
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

    #[error(transparent)]
    RetryExhausted(#[from] crate::util::retry::RetryExhausted),
}

impl crate::error::ErrorClassification for A2aClientError {
    fn category(&self) -> crate::error::ErrorCategory {
        use crate::error::ErrorCategory;
        match self {
            A2aClientError::ClientBuild { .. } => ErrorCategory::Internal,
            A2aClientError::RequestFailed { source, .. } => {
                if source.is_timeout() {
                    ErrorCategory::Timeout
                } else if source.is_connect() || source.is_request() {
                    ErrorCategory::Network
                } else {
                    ErrorCategory::Provider
                }
            }
            A2aClientError::AgentNotFound { .. } | A2aClientError::TaskNotFound { .. } => {
                ErrorCategory::NotFound
            }
            A2aClientError::AgentListFailed { status, .. }
            | A2aClientError::TaskSendFailed { status, .. }
            | A2aClientError::TaskQueryFailed { status, .. }
            | A2aClientError::TaskCancelFailed { status, .. } => {
                if *status == 429 {
                    ErrorCategory::RateLimit
                } else if *status == 401 || *status == 403 {
                    ErrorCategory::Permission
                } else if (500..600).contains(status) {
                    ErrorCategory::Provider
                } else {
                    ErrorCategory::Validation
                }
            }
            A2aClientError::InvalidUrl { .. } | A2aClientError::SsrfBlocked { .. } => {
                ErrorCategory::Validation
            }
            A2aClientError::InvalidResponse { .. } => ErrorCategory::Provider,
            A2aClientError::PollingTimeout { .. } => ErrorCategory::Timeout,
            A2aClientError::RetryExhausted(inner) => inner.category(),
        }
    }
}

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
