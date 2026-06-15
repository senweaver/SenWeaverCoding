// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::AppState;
use crate::runtime::task_manager::spawn_supervised;
use axum::{
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, header},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

const BEARER_SUBPROTO_PREFIX: &str = "bearer.";

const WS_NODE_PROTOCOL: &str = "sen.nodes.v1";

const NODE_INVOCATION_TIMEOUT: Duration = Duration::from_secs(60);

const NODE_PENDING_SWEEP_INTERVAL: Duration = Duration::from_secs(5);

struct PendingInvocation {
    response_tx: oneshot::Sender<NodeInvocationResult>,
    deadline: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapability {
    pub name: String,
    pub description: String,
    #[serde(default = "default_capability_parameters")]
    pub parameters: serde_json::Value,
}

fn default_capability_parameters() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {}
    })
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: String,
    pub capabilities: Vec<NodeCapability>,

    pub invoke_tx: mpsc::Sender<NodeInvocation>,
}

#[derive(Debug)]
pub struct NodeInvocation {
    pub call_id: String,
    pub capability: String,
    pub args: serde_json::Value,
    pub response_tx: oneshot::Sender<NodeInvocationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInvocationResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct NodeRegistry {
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    max_nodes: usize,
}

impl NodeRegistry {

    pub fn new(max_nodes: usize) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            max_nodes,
        }
    }

    pub fn register(&self, info: NodeInfo) -> bool {
        let mut nodes = self.nodes.write();
        if nodes.len() >= self.max_nodes && !nodes.contains_key(&info.node_id) {
            return false;
        }
        nodes.insert(info.node_id.clone(), info);
        true
    }

    pub fn unregister(&self, node_id: &str) {
        self.nodes.write().remove(node_id);
    }

    pub fn node_ids(&self) -> Vec<String> {
        self.nodes.read().keys().cloned().collect()
    }

    pub fn all_capabilities(&self) -> Vec<(String, String, NodeCapability)> {
        let nodes = self.nodes.read();
        let mut caps = Vec::new();
        for info in nodes.values() {
            for cap in &info.capabilities {
                caps.push((info.node_id.clone(), cap.name.clone(), cap.clone()));
            }
        }
        caps
    }

    pub fn invoke_tx(&self, node_id: &str) -> Option<mpsc::Sender<NodeInvocation>> {
        self.nodes.read().get(node_id).map(|n| n.invoke_tx.clone())
    }

    pub fn contains(&self, node_id: &str) -> bool {
        self.nodes.read().contains_key(node_id)
    }

    pub fn len(&self) -> usize {
        self.nodes.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.read().is_empty()
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NodeMessage {
    Register {
        node_id: String,
        capabilities: Vec<NodeCapability>,
    },
    Result {
        call_id: String,
        success: bool,
        output: String,
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
enum GatewayMessage {
    Registered {
        node_id: String,
        capabilities_count: usize,
    },
    Error {
        message: String,
    },
    Invoke {
        call_id: String,
        capability: String,
        args: serde_json::Value,
    },
}

#[derive(Deserialize)]
pub struct NodeWsQuery {
    pub token: Option<String>,
}

fn extract_node_ws_token<'a>(
    headers: &'a HeaderMap,
    query_token: Option<&'a str>,
) -> Option<&'a str> {

    if let Some(t) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
    {
        if !t.is_empty() {
            return Some(t);
        }
    }

    if let Some(t) = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .and_then(|protos| {
            protos
                .split(',')
                .map(|p| p.trim())
                .find_map(|p| p.strip_prefix(BEARER_SUBPROTO_PREFIX))
        })
    {
        if !t.is_empty() {
            return Some(t);
        }
    }

    if let Some(t) = query_token {
        if !t.is_empty() {
            return Some(t);
        }
    }

    None
}

pub async fn handle_ws_nodes(
    State(state): State<AppState>,
    Query(params): Query<NodeWsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {

    let nodes_config = state.config.lock().nodes.clone();
    if let Some(ref expected_token) = nodes_config.auth_token {
        let token = extract_node_ws_token(&headers, params.token.as_deref()).unwrap_or("");
        if token != expected_token {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized  -  provide a valid node auth token",
            )
                .into_response();
        }
    }

    if nodes_config.auth_token.is_none() && state.pairing.require_pairing() {
        let token = extract_node_ws_token(&headers, params.token.as_deref()).unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized  -  provide Authorization header or ?token= query param",
            )
                .into_response();
        }
    }

    let ws = if headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .map_or(false, |protos| {
            protos.split(',').any(|p| p.trim() == WS_NODE_PROTOCOL)
        }) {
        ws.protocols([WS_NODE_PROTOCOL])
    } else {
        ws
    };

    let registry = state.node_registry.clone();
    ws.on_upgrade(move |socket| handle_node_socket(socket, registry))
        .into_response()
}

async fn handle_node_socket(socket: WebSocket, registry: Arc<NodeRegistry>) {
    let (mut sender, mut receiver) = socket.split();
    let mut registered_node_id: Option<String> = None;

    let (invoke_tx, mut invoke_rx) = mpsc::channel::<NodeInvocation>(32);
    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<String>(16);

    let pending: Arc<RwLock<HashMap<String, PendingInvocation>>> =
        Arc::new(RwLock::new(HashMap::new()));

    let pending_clone = Arc::clone(&pending);

    let send_task = spawn_supervised("gateway.nodes.invocation_forwarder", async move {
        loop {
            tokio::select! {
                maybe_invocation = invoke_rx.recv() => {
                    let Some(invocation) = maybe_invocation else { break; };
                    let msg = GatewayMessage::Invoke {
                        call_id: invocation.call_id.clone(),
                        capability: invocation.capability,
                        args: invocation.args,
                    };
                    if let Ok(json) = serde_json::to_string(&msg) {
                        pending_clone.write().insert(
                            invocation.call_id.clone(),
                            PendingInvocation {
                                response_tx: invocation.response_tx,
                                deadline: Instant::now() + NODE_INVOCATION_TIMEOUT,
                            },
                        );
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            pending_clone.write().remove(&invocation.call_id);
                            break;
                        }
                    }
                }
                maybe_ctrl = ctrl_rx.recv() => {
                    let Some(json) = maybe_ctrl else { break; };
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let sweep_pending = Arc::clone(&pending);
    let sweep_task = spawn_supervised("gateway.nodes.pending_sweeper", async move {
        let mut ticker = tokio::time::interval(NODE_PENDING_SWEEP_INTERVAL);
        loop {
            ticker.tick().await;
            let now = Instant::now();
            let expired: Vec<String> = {
                let map = sweep_pending.read();
                map.iter()
                    .filter(|(_, entry)| entry.deadline <= now)
                    .map(|(call_id, _)| call_id.clone())
                    .collect()
            };
            if expired.is_empty() {
                continue;
            }
            let mut map = sweep_pending.write();
            for call_id in expired {
                if let Some(entry) = map.remove(&call_id) {
                    let _ = entry.response_tx.send(NodeInvocationResult {
                        success: false,
                        output: String::new(),
                        error: Some("node invocation timed out".to_string()),
                    });
                    tracing::warn!(
                        call_id = %call_id,
                        "node invocation timed out; cleaned up pending entry"
                    );
                }
            }
        }
    });

    while let Some(msg) = receiver.next().await {
        let text = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) | Err(_) => break,
            _ => continue,
        };

        let parsed: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let node_msg: NodeMessage = match serde_json::from_value(parsed) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match node_msg {
            NodeMessage::Register {
                node_id,
                capabilities,
            } => {

                if node_id.is_empty() || node_id.len() > 128 {
                    tracing::warn!("Node registration rejected: invalid node_id length");
                    continue;
                }

                let caps_count = capabilities.len();
                let info = NodeInfo {
                    node_id: node_id.clone(),
                    capabilities,
                    invoke_tx: invoke_tx.clone(),
                };

                if registry.register(info) {
                    tracing::info!("Node registered: {node_id} with {caps_count} capabilities");
                    registered_node_id = Some(node_id.clone());

                    if let Ok(json) = serde_json::to_string(&GatewayMessage::Registered {
                        node_id: node_id.clone(),
                        capabilities_count: caps_count,
                    }) {
                        let _ = ctrl_tx.send(json).await;
                    }
                } else {
                    tracing::warn!(
                        "Node registration rejected: registry at capacity for {node_id}"
                    );
                    if let Ok(json) = serde_json::to_string(&GatewayMessage::Error {
                        message: format!("node registry at capacity; rejected `{node_id}`"),
                    }) {
                        let _ = ctrl_tx.send(json).await;
                    }
                }
            }
            NodeMessage::Result {
                call_id,
                success,
                output,
                error,
            } => {
                if let Some(entry) = pending.write().remove(&call_id) {
                    let _ = entry.response_tx.send(NodeInvocationResult {
                        success,
                        output,
                        error,
                    });
                }
            }
        }
    }

    if let Some(node_id) = registered_node_id {
        registry.unregister(&node_id);
        tracing::info!("Node disconnected and unregistered: {node_id}");
    }

    send_task.abort();
    sweep_task.abort();
}
