// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Live Canvas (A2UI) tool — push rendered content to a web canvas in real time.
//!
//! The agent can render HTML/SVG/Markdown to a named canvas, snapshot its
//! current state, clear it, or evaluate a JavaScript expression in the canvas
//! context. Content is stored in a shared [`CanvasStore`] and broadcast to
//! connected WebSocket clients via per-canvas channels.

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

pub const MAX_CONTENT_SIZE: usize = 256 * 1024;

const MAX_HISTORY_FRAMES: usize = 50;

const BROADCAST_CAPACITY: usize = 64;

const MAX_CANVAS_COUNT: usize = 100;

pub const ALLOWED_CONTENT_TYPES: &[&str] = &["html", "svg", "markdown", "text"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasFrame {

    pub frame_id: String,

    pub content_type: String,

    pub content: String,

    pub timestamp: String,
}

struct CanvasEntry {
    current: Option<CanvasFrame>,
    history: Vec<CanvasFrame>,
    tx: broadcast::Sender<CanvasFrame>,
}

#[derive(Clone)]
pub struct CanvasStore {
    inner: Arc<RwLock<HashMap<String, CanvasEntry>>>,
}

impl Default for CanvasStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CanvasStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn render(
        &self,
        canvas_id: &str,
        content_type: &str,
        content: &str,
    ) -> Option<CanvasFrame> {
        let frame = CanvasFrame {
            frame_id: uuid::Uuid::new_v4().to_string(),
            content_type: content_type.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        let mut store = self.inner.write();

        if !store.contains_key(canvas_id) && store.len() >= MAX_CANVAS_COUNT {
            return None;
        }

        let entry = store
            .entry(canvas_id.to_string())
            .or_insert_with(|| CanvasEntry {
                current: None,
                history: Vec::new(),
                tx: broadcast::channel(BROADCAST_CAPACITY).0,
            });

        entry.current = Some(frame.clone());
        entry.history.push(frame.clone());
        if entry.history.len() > MAX_HISTORY_FRAMES {
            let excess = entry.history.len() - MAX_HISTORY_FRAMES;
            entry.history.drain(..excess);
        }

        let _ = entry.tx.send(frame.clone());

        Some(frame)
    }

    pub fn snapshot(&self, canvas_id: &str) -> Option<CanvasFrame> {
        let store = self.inner.read();
        store.get(canvas_id).and_then(|entry| entry.current.clone())
    }

    pub fn history(&self, canvas_id: &str) -> Vec<CanvasFrame> {
        let store = self.inner.read();
        store
            .get(canvas_id)
            .map(|entry| entry.history.clone())
            .unwrap_or_default()
    }

    pub fn clear(&self, canvas_id: &str) -> bool {
        let mut store = self.inner.write();
        if let Some(entry) = store.get_mut(canvas_id) {
            entry.current = None;
            entry.history.clear();

            let clear_frame = CanvasFrame {
                frame_id: uuid::Uuid::new_v4().to_string(),
                content_type: "clear".to_string(),
                content: String::new(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            let _ = entry.tx.send(clear_frame);
            true
        } else {
            false
        }
    }

    pub fn subscribe(&self, canvas_id: &str) -> Option<broadcast::Receiver<CanvasFrame>> {
        let mut store = self.inner.write();

        if !store.contains_key(canvas_id) && store.len() >= MAX_CANVAS_COUNT {
            return None;
        }

        let entry = store
            .entry(canvas_id.to_string())
            .or_insert_with(|| CanvasEntry {
                current: None,
                history: Vec::new(),
                tx: broadcast::channel(BROADCAST_CAPACITY).0,
            });
        Some(entry.tx.subscribe())
    }

    pub fn list(&self) -> Vec<String> {
        let store = self.inner.read();
        store.keys().cloned().collect()
    }
}

pub struct CanvasTool {
    store: CanvasStore,
}

impl CanvasTool {
    pub fn new(store: CanvasStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for CanvasTool {
    fn name(&self) -> &str {
        "canvas"
    }

    fn description(&self) -> &str {
        "Push rendered content (HTML, SVG, Markdown) to a live web canvas that users can see \
         in real-time. Actions: render (push content), snapshot (get current content), \
         clear (reset canvas), eval (evaluate JS expression in canvas context). \
         Each canvas is identified by a canvas_id string."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform on the canvas.",
                    "enum": ["render", "snapshot", "clear", "eval"]
                },
                "canvas_id": {
                    "type": "string",
                    "description": "Unique identifier for the canvas. Defaults to 'default'."
                },
                "content_type": {
                    "type": "string",
                    "description": "Content type for render action: html, svg, markdown, or text.",
                    "enum": ["html", "svg", "markdown", "text"]
                },
                "content": {
                    "type": "string",
                    "description": "Content to render (for render action)."
                },
                "expression": {
                    "type": "string",
                    "description": "JavaScript expression to evaluate (for eval action). \
                        The result is returned as text. Evaluated client-side in the canvas iframe."
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
                    error: Some("Missing required parameter: action".to_string()),
                });
            }
        };

        let canvas_id = args
            .get("canvas_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        match action {
            "render" => {
                let content_type = args
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("html");

                let content = match args.get("content").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(
                                "Missing required parameter: content (for render action)"
                                    .to_string(),
                            ),
                        });
                    }
                };

                if content.len() > MAX_CONTENT_SIZE {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Content exceeds maximum size of {} bytes",
                            MAX_CONTENT_SIZE
                        )),
                    });
                }

                match self.store.render(canvas_id, content_type, content) {
                    Some(frame) => Ok(ToolResult {
                        success: true,
                        output: format!(
                            "Rendered {} content to canvas '{}' (frame: {})",
                            content_type, canvas_id, frame.frame_id
                        ),
                        error: None,
                    }),
                    None => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Maximum canvas count ({}) reached. Clear unused canvases first.",
                            MAX_CANVAS_COUNT
                        )),
                    }),
                }
            }

            "snapshot" => match self.store.snapshot(canvas_id) {
                Some(frame) => Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string_pretty(&frame)
                        .unwrap_or_else(|_| frame.content.clone()),
                    error: None,
                }),
                None => Ok(ToolResult {
                    success: true,
                    output: format!("Canvas '{}' is empty", canvas_id),
                    error: None,
                }),
            },

            "clear" => {
                let existed = self.store.clear(canvas_id);
                Ok(ToolResult {
                    success: true,
                    output: if existed {
                        format!("Canvas '{}' cleared", canvas_id)
                    } else {
                        format!("Canvas '{}' was already empty", canvas_id)
                    },
                    error: None,
                })
            }

            "eval" => {

                let expression = match args.get("expression").and_then(|v| v.as_str()) {
                    Some(e) => e,
                    None => {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(
                                "Missing required parameter: expression (for eval action)"
                                    .to_string(),
                            ),
                        });
                    }
                };

                match self.store.render(canvas_id, "eval", expression) {
                    Some(frame) => Ok(ToolResult {
                        success: true,
                        output: format!(
                            "Eval request sent to canvas '{}' (frame: {}). \
                             Result will be available to connected viewers.",
                            canvas_id, frame.frame_id
                        ),
                        error: None,
                    }),
                    None => Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(format!(
                            "Maximum canvas count ({}) reached. Clear unused canvases first.",
                            MAX_CANVAS_COUNT
                        )),
                    }),
                }
            }

            other => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Unknown action: '{}'. Valid actions: render, snapshot, clear, eval",
                    other
                )),
            }),
        }
    }
}
