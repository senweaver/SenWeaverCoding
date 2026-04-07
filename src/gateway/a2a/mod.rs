// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! A2A Protocol (Agent-to-Agent) - Standardized agent communication protocol.
//!
//! This module implements the A2A protocol for inter-agent communication.
//! It provides:
//! - Agent discovery via `/.well-known/agent.json`
//! - Task submission and status tracking
//! - Standardized message formats
//!
//! # Endpoints
//!
//! Public A2A endpoints:
//! - `GET /.well-known/agent.json`  - Agent card discovery
//! - `GET /a2a/agents`  - List available agents
//! - `POST /a2a/tasks/send`  - Submit a new task
//! - `GET /a2a/tasks/{id}`  - Get task status
//! - `POST /a2a/tasks/{id}/cancel`  - Cancel a task
//!
//! Admin endpoints:
//! - `GET /api/a2a/agents`  - List all known agents
//! - `POST /api/a2a/discover`  - Discover external agents

pub mod client;
pub mod routes;
pub mod types;

// Re-export main types for convenient access
#[allow(unused_imports)]
pub use client::{A2aClient, A2aClientError, discover_external_agents};
#[allow(unused_imports)]
pub use routes::{A2aState, TaskExecutor, build_a2a_state, create_a2a_router};
#[allow(unused_imports)]
pub use types::{
    A2aError, A2aTask, A2aTaskStore, AgentAuth, AgentCapabilities, AgentCard, CancelTaskRequest,
    CancelTaskResponse, DiscoverAgentRequest, ListAgentsResponse, SendTaskRequest,
    SendTaskResponse, TaskId, TaskResult, TaskStatus,
};
