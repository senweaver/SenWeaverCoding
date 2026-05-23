// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use crate::runtime::task_manager::spawn_supervised;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use crate::gateway::a2a::types::{
    A2aError, A2aTask, A2aTaskStore, AgentCard, CancelTaskRequest, CancelTaskResponse,
    ListAgentsResponse, SendTaskRequest, SendTaskResponse, TaskId, TaskResult,
};

pub type TaskExecutor =
    Arc<dyn Fn(String) -> tokio::task::JoinHandle<Result<String, String>> + Send + Sync>;

#[derive(Clone)]
pub struct A2aState {

    pub agent_card: AgentCard,

    pub task_store: Arc<Mutex<A2aTaskStore>>,

    pub external_agents: Arc<Mutex<Vec<(String, AgentCard)>>>,

    pub task_executor: Option<TaskExecutor>,
}

impl A2aState {

    pub fn new(agent_card: AgentCard) -> Self {
        Self {
            agent_card,
            task_store: Arc::new(Mutex::new(A2aTaskStore::new())),
            external_agents: Arc::new(Mutex::new(Vec::new())),
            task_executor: None,
        }
    }

    pub fn with_executor(mut self, executor: TaskExecutor) -> Self {
        self.task_executor = Some(executor);
        self
    }

    pub fn set_external_agents(&self, agents: Vec<(String, AgentCard)>) {
        let mut ext = self.external_agents.lock();
        *ext = agents;
    }

    pub fn create_task(&self, request: SendTaskRequest) -> A2aTask {
        let task = request.into_task();
        let mut store = self.task_store.lock();
        store.cleanup_old(chrono::Duration::hours(24));
        store.store(task.clone());
        task
    }

    pub fn get_task(&self, id: &TaskId) -> Option<A2aTask> {
        let store = self.task_store.lock();
        store.get(id).cloned()
    }

    pub fn update_task(&self, task: A2aTask) {
        let mut store = self.task_store.lock();
        store.update(task);
    }
}

pub async fn get_agent_card(State(state): State<A2aState>) -> impl IntoResponse {
    Json(state.agent_card.clone())
}

pub async fn list_agents(State(state): State<A2aState>) -> impl IntoResponse {
    let external = state.external_agents.lock();

    let mut agents = vec![state.agent_card.clone()];
    for (_, card) in external.iter() {
        agents.push(card.clone());
    }

    let response = ListAgentsResponse {
        total: agents.len(),
        agents,
    };

    Json(response)
}

pub async fn send_task(
    State(state): State<A2aState>,
    Json(request): Json<SendTaskRequest>,
) -> impl IntoResponse {
    if request.name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(A2aError::invalid_request("Task name is required")),
        )
            .into_response();
    }

    let callback_url = request.callback_url.clone();
    let task = state.create_task(request);
    let task_id = task.id.clone();

    tracing::info!(task_id = %task_id, name = %task.name, "A2A task received");

    if let Some(ref executor) = state.task_executor {
        let mut working_task = task.clone();
        working_task.mark_working();
        state.update_task(working_task);

        let executor = Arc::clone(executor);
        let state_clone = state.clone();
        let description = task.description.clone();
        let tid = task_id.clone();

        spawn_supervised("gateway.a2a.task_executor", async move {
            let handle = executor(description);
            match handle.await {
                Ok(Ok(result_text)) => {
                    if let Some(mut t) = state_clone.get_task(&tid) {
                        if !t.is_terminal() {
                            t.mark_completed(TaskResult::Text { text: result_text });
                            state_clone.update_task(t.clone());
                            tracing::info!(task_id = %tid, "A2A task completed");
                            notify_callback(callback_url.as_deref(), &t).await;
                        }
                    }
                }
                Ok(Err(err_str)) => {
                    if let Some(mut t) = state_clone.get_task(&tid) {
                        if !t.is_terminal() {
                            t.mark_failed(&err_str);
                            state_clone.update_task(t.clone());
                            tracing::error!(task_id = %tid, error = %err_str, "A2A task failed");
                            notify_callback(callback_url.as_deref(), &t).await;
                        }
                    }
                }
                Err(join_err) => {
                    let err_msg = format!("Task execution panicked: {join_err}");
                    if let Some(mut t) = state_clone.get_task(&tid) {
                        if !t.is_terminal() {
                            t.mark_failed(&err_msg);
                            state_clone.update_task(t.clone());
                            tracing::error!(task_id = %tid, error = %err_msg, "A2A task failed");
                            notify_callback(callback_url.as_deref(), &t).await;
                        }
                    }
                }
            }
        });

        let response = SendTaskResponse {
            task: state.get_task(&task_id).unwrap_or(task),
            estimated_completion_secs: Some(120),
        };
        (StatusCode::ACCEPTED, Json(response)).into_response()
    } else {
        let response = SendTaskResponse {
            task,
            estimated_completion_secs: None,
        };
        (StatusCode::CREATED, Json(response)).into_response()
    }
}

pub async fn get_task(State(state): State<A2aState>, Path(id): Path<TaskId>) -> impl IntoResponse {
    match state.get_task(&id) {
        Some(task) => (StatusCode::OK, Json(task)).into_response(),
        None => (StatusCode::NOT_FOUND, Json(A2aError::task_not_found(&id))).into_response(),
    }
}

pub async fn cancel_task(
    State(state): State<A2aState>,
    Path(id): Path<TaskId>,
    Json(request): Json<CancelTaskRequest>,
) -> impl IntoResponse {
    match state.get_task(&id) {
        Some(mut task) => {
            if task.is_terminal() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(A2aError::invalid_request(format!(
                        "Task '{}' is already in terminal state: {}",
                        id, task.status
                    ))),
                )
                    .into_response();
            }

            task.mark_cancelled();
            if let Some(reason) = request.reason {
                task.metadata
                    .insert("cancel_reason".to_string(), serde_json::json!(reason));
            }

            state.update_task(task.clone());

            tracing::info!(task_id = %id, "A2A task cancelled");

            let response = CancelTaskResponse {
                task,
                success: true,
            };

            (StatusCode::OK, Json(response)).into_response()
        }
        None => (StatusCode::NOT_FOUND, Json(A2aError::task_not_found(&id))).into_response(),
    }
}

async fn notify_callback(url: Option<&str>, task: &A2aTask) {
    let Some(url) = url else { return };
    let client = reqwest::Client::new();
    if let Err(e) = client
        .post(url)
        .json(task)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        tracing::warn!(url = %url, error = %e, "A2A callback notification failed");
    }
}

pub async fn discover_external(
    State(state): State<A2aState>,
    Json(urls): Json<Vec<String>>,
) -> impl IntoResponse {
    tracing::info!("Discovering {} external A2A agents", urls.len());

    let client = crate::gateway::a2a::client::A2aClient::new();
    let mut results: HashMap<String, serde_json::Value> = HashMap::new();

    for url in &urls {
        match client.discover_agent(url).await {
            Ok(card) => {
                let mut ext = state.external_agents.lock();
                ext.push((url.clone(), card.clone()));
                results.insert(
                    url.clone(),
                    serde_json::json!({
                        "status": "discovered",
                        "agent": card,
                    }),
                );
            }
            Err(e) => {
                results.insert(
                    url.clone(),
                    serde_json::json!({
                        "status": "failed",
                        "error": format!("{e}"),
                    }),
                );
            }
        }
    }

    Json(results)
}

pub async fn list_all_agents(State(state): State<A2aState>) -> impl IntoResponse {
    let external = state.external_agents.lock();

    let agents: Vec<serde_json::Value> = std::iter::once(serde_json::json!({
        "id": state.agent_card.id,
        "name": state.agent_card.name,
        "url": state.agent_card.url,
        "is_local": true,
    }))
    .chain(external.iter().map(|(url, card)| {
        serde_json::json!({
            "id": card.id,
            "name": card.name,
            "url": url,
            "is_local": false,
        })
    }))
    .collect();

    Json(serde_json::json!({ "agents": agents, "total": agents.len() }))
}

pub fn create_a2a_router(state: A2aState) -> Router {

    let public_routes = Router::new()
        .route("/.well-known/agent.json", get(get_agent_card))
        .route("/a2a/agents", get(list_agents))
        .route("/a2a/tasks/send", post(send_task))
        .route("/a2a/tasks/{id}", get(get_task))
        .route("/a2a/tasks/{id}/cancel", post(cancel_task));

    let admin_routes = Router::new()
        .route("/api/a2a/agents", get(list_all_agents))
        .route("/api/a2a/discover", post(discover_external));

    public_routes.merge(admin_routes).with_state(state)
}

pub fn build_a2a_state(agent_name: impl Into<String>, base_url: impl Into<String>) -> A2aState {
    let agent_card = AgentCard::build_agent_card(agent_name, base_url, vec![]);
    A2aState::new(agent_card)
}
