// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod client;
pub mod routes;
pub mod types;
pub use client::{A2aClient, A2aClientError, discover_external_agents};
pub use routes::{A2aState, TaskExecutor, build_a2a_state, create_a2a_router};
pub use types::{
    A2aError, A2aTask, A2aTaskStore, AgentAuth, AgentCapabilities, AgentCard, CancelTaskRequest,
    CancelTaskResponse, DiscoverAgentRequest, ListAgentsResponse, SendTaskRequest,
    SendTaskResponse, TaskId, TaskResult, TaskStatus,
};
