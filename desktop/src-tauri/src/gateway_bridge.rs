// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use senweavercoding::tools::browser::{DockRequest, dock_controller};
use senweavercoding::tools::web::fetch::fetch_controller;
use serde_json::{Value, json};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

static BRIDGE_GENERATION: AtomicU64 = AtomicU64::new(0);

const DISPATCH_MAX_CONCURRENCY: usize = 32;

const DISPATCH_DEADLINE: Duration = Duration::from_secs(120);

pub fn next_bridge_generation() -> u64 {
    BRIDGE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

fn current_bridge_generation() -> u64 {
    BRIDGE_GENERATION.load(Ordering::SeqCst)
}

pub fn spawn_bridge_client(gateway_url: String, token: String, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let ws_base = gateway_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        let ws_url = format!("{ws_base}/ws/desktop-bridge?token={token}");
        let mut backoff_ms: u64 = 500;
        loop {
            if current_bridge_generation() != generation {
                tracing::info!(
                    "[gateway_bridge] generation superseded; stopping bridge client"
                );
                return;
            }
            match connect_async(&ws_url).await {
                Ok((stream, _)) => {
                    backoff_ms = 500;
                    tracing::info!("[gateway_bridge] connected to gateway bridge");
                    run_bridge_session(stream, generation).await;
                    tracing::warn!("[gateway_bridge] bridge session ended");
                }
                Err(err) => {
                    tracing::debug!("[gateway_bridge] connect failed: {err}");
                }
            }
            if current_bridge_generation() != generation {
                return;
            }
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(10_000);
        }
    });
}

async fn run_bridge_session(
    stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    generation: u64,
) {
    let (sink, mut reader) = stream.split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<String>(256);

    let writer = tauri::async_runtime::spawn(async move {
        let mut sink = sink;
        while let Some(frame) = out_rx.recv().await {
            if sink.send(Message::Text(frame.into())).await.is_err() {
                break;
            }
        }
    });

    let dispatch_gate = std::sync::Arc::new(tokio::sync::Semaphore::new(
        DISPATCH_MAX_CONCURRENCY,
    ));

    while let Some(message) = reader.next().await {
        if current_bridge_generation() != generation {
            break;
        }
        let text = match message {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        let frame = match serde_json::from_str::<Value>(&text) {
            Ok(frame) => frame,
            Err(err) => {
                tracing::warn!(
                    "[gateway_bridge] dropping unparseable bridge frame (no id recoverable): {err}"
                );
                continue;
            }
        };
        let Some(id) = frame.get("id").and_then(Value::as_u64) else {
            tracing::warn!("[gateway_bridge] dropping bridge frame without a numeric id");
            continue;
        };
        let target = frame
            .get("target")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let (Some(target), Some(method)) = (target, method) else {
            let response = json!({ "id": id, "ok": false, "error": "malformed frame" });
            if out_tx.send(response.to_string()).await.is_err() {
                tracing::error!(
                    "[gateway_bridge] failed to queue malformed-frame response for request {id}: outbound channel closed"
                );
            }
            continue;
        };
        let args = frame.get("args").cloned().unwrap_or(Value::Null);
        let permit = match dispatch_gate.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };
        let out_tx = out_tx.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let result =
                match tokio::time::timeout(DISPATCH_DEADLINE, dispatch(&target, &method, args))
                    .await
                {
                    Ok(result) => result,
                    Err(_) => Err(format!(
                        "bridge dispatch '{target}.{method}' exceeded {}s deadline",
                        DISPATCH_DEADLINE.as_secs()
                    )),
                };
            let response = match result {
                Ok(value) => json!({ "id": id, "ok": true, "value": value }),
                Err(err) => json!({ "id": id, "ok": false, "error": err }),
            };
            if out_tx.send(response.to_string()).await.is_err() {
                tracing::error!(
                    "[gateway_bridge] failed to queue bridge response for request {id}: outbound channel closed"
                );
            }
        });
    }

    writer.abort();
}

async fn dispatch(target: &str, method: &str, args: Value) -> Result<Value, String> {
    match target {
        "dock" => dispatch_dock(method, args).await,
        "fetch" => dispatch_fetch(method, args).await,
        other => Err(format!("unknown bridge target: {other}")),
    }
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn arg_u32(args: &Value, key: &str) -> Result<u32, String> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .ok_or_else(|| format!("bridge request missing `{key}`"))
}

async fn dispatch_dock(method: &str, args: Value) -> Result<Value, String> {
    let controller =
        dock_controller().ok_or_else(|| "dock controller unavailable in desktop".to_string())?;
    match method {
        "ensure_visible" => {
            controller
                .ensure_visible(arg_str(&args, "session_hint"))
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "exec" => {
            let req = DockRequest {
                kind: arg_str(&args, "kind").unwrap_or_default(),
                args: args.get("args").cloned().unwrap_or(Value::Null),
                timeout_ms: args
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(10_000),
            };
            let resp = controller.exec(req).await.map_err(|e| e.to_string())?;
            Ok(json!({ "ok": resp.ok, "value": resp.value, "error": resp.error }))
        }
        "screenshot" => {
            let full_page = args
                .get("full_page")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let bytes = controller
                .screenshot(full_page)
                .await
                .map_err(|e| e.to_string())?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Ok(json!({ "png_base64": encoded }))
        }
        "new_tab" => {
            let activate = args
                .get("activate")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let tab_id = controller
                .new_tab(arg_str(&args, "url"), activate)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "tab_id": tab_id }))
        }
        "close_tab" => {
            let active = controller
                .close_tab(arg_u32(&args, "tab_id")?)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "active": active }))
        }
        "activate_tab" => {
            controller
                .activate_tab(arg_u32(&args, "tab_id")?)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "list_tabs" => {
            let tabs = controller.list_tabs().await.map_err(|e| e.to_string())?;
            Ok(json!({ "tabs": tabs }))
        }
        "bind_tab_to_session" => {
            controller
                .bind_tab_to_session(
                    arg_str(&args, "session_id").unwrap_or_default(),
                    arg_u32(&args, "tab_id")?,
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "unbind_tab_from_session" => {
            controller
                .unbind_tab_from_session(
                    arg_str(&args, "session_id").unwrap_or_default(),
                    arg_u32(&args, "tab_id")?,
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        "release_agent_tabs_for_session" => {
            let tabs = controller
                .release_agent_tabs_for_session(
                    arg_str(&args, "session_id").unwrap_or_default(),
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "tabs": tabs }))
        }
        "present_session" => {
            let tab = controller
                .present_session(arg_str(&args, "session_id").unwrap_or_default())
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "tab": tab }))
        }
        "park" => {
            controller.park().await.map_err(|e| e.to_string())?;
            Ok(json!({}))
        }
        other => Err(format!("unknown dock bridge method: {other}")),
    }
}

async fn dispatch_fetch(method: &str, args: Value) -> Result<Value, String> {
    let controller = fetch_controller()
        .ok_or_else(|| "fetch controller unavailable in desktop".to_string())?;
    match method {
        "fetch" => {
            let url = arg_str(&args, "url").ok_or("bridge fetch missing `url`")?;
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(30_000);
            let page = controller
                .fetch(&url, Duration::from_millis(timeout_ms.max(1_000)))
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "url": page.url, "title": page.title, "text": page.text }))
        }
        other => Err(format!("unknown fetch bridge method: {other}")),
    }
}
