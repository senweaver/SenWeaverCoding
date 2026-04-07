// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! JSON-RPC 2.0 codec types.
//!
//! Handles serialization/deserialization of JSON-RPC 2.0 request, response,
//! notification, and error objects.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request or notification.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawJsonRpcRequest")]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    pub id: Option<Value>,
}

impl JsonRpcRequest {
    /// Returns `true` if this is a notification (no id).
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// Intermediate representation used for custom deserialization.
#[derive(Deserialize)]
struct RawJsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    id: Option<Value>,
}

impl TryFrom<RawJsonRpcRequest> for JsonRpcRequest {
    type Error = &'static str;

    fn try_from(raw: RawJsonRpcRequest) -> Result<Self, Self::Error> {
        if raw.jsonrpc != "2.0" {
            return Err("jsonrpc version must be \"2.0\"");
        }
        Ok(JsonRpcRequest {
            jsonrpc: raw.jsonrpc,
            method: raw.method,
            params: raw.params,
            id: raw.id,
        })
    }
}

/// JSON-RPC 2.0 response (success or error).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: Value,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: Value, err: RpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(err),
            id,
        }
    }
}

/// JSON-RPC 2.0 notification (no id, no response expected).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcNotification {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: Value,
}

impl JsonRpcNotification {
    pub fn new(method: &'static str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params,
        }
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: impl Into<i32>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

// ── Standard JSON-RPC 2.0 error codes ────────────────────────────────────────

/// Parse error. Invalid JSON was received.
pub const PARSE_ERROR: i32 = -32700;
/// Invalid Request. The JSON sent is not a valid Request object.
pub const INVALID_REQUEST: i32 = -32600;
/// Method not found. The method does not exist or is not available.
pub const METHOD_NOT_FOUND: i32 = -32601;
/// Invalid method parameter(s).
pub const INVALID_PARAMS: i32 = -32602;
/// Internal JSON-RPC error.
pub const INTERNAL_ERROR: i32 = -32603;

/// Alias for the standard `RpcError` constructor using [`PARSE_ERROR`].
pub type RpcErrorCode = i32;

impl RpcError {
    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(PARSE_ERROR, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message)
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("Method not found: {method}"))
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(INTERNAL_ERROR, message)
    }

    // ── Custom SenWeaverCoding error codes ──────────────────────────────────────

    /// Session not found.
    pub const SESSION_NOT_FOUND: i32 = -32000;
    /// Session limit reached.
    pub const SESSION_LIMIT_REACHED: i32 = -32001;
    /// Session timeout.
    pub const SESSION_TIMEOUT: i32 = -32002;
    /// Agent internal error.
    pub const AGENT_ERROR: i32 = -32003;
    /// Tool execution error.
    pub const TOOL_ERROR: i32 = -32004;
    /// Memory error.
    pub const MEMORY_ERROR: i32 = -32005;
    /// Blackboard error.
    pub const BLACKBOARD_ERROR: i32 = -32006;
    /// Authorization error.
    pub const AUTH_ERROR: i32 = -32007;

    pub fn session_not_found(id: &str) -> Self {
        Self::new(Self::SESSION_NOT_FOUND, format!("Session not found: {id}"))
    }

    pub fn session_limit_reached(max: usize) -> Self {
        Self::new(
            Self::SESSION_LIMIT_REACHED,
            format!("Maximum session limit reached ({max})"),
        )
    }

    pub fn session_timeout(id: &str) -> Self {
        Self::new(Self::SESSION_TIMEOUT, format!("Session timed out: {id}"))
    }

    pub fn agent(message: impl Into<String>) -> Self {
        Self::new(Self::AGENT_ERROR, message)
    }

    pub fn tool(name: &str, message: impl Into<String>) -> Self {
        Self::new(
            Self::TOOL_ERROR,
            format!("Tool '{name}' error: {}", message.into()),
        )
    }

    pub fn memory(message: impl Into<String>) -> Self {
        Self::new(Self::MEMORY_ERROR, message)
    }

    pub fn blackboard(message: impl Into<String>) -> Self {
        Self::new(Self::BLACKBOARD_ERROR, message)
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(Self::AUTH_ERROR, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_parse_with_id() {
        let json = r#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "initialize");
        assert!(!req.is_notification());
        assert_eq!(req.id.as_ref().unwrap(), &Value::Number(1.into()));
    }

    #[test]
    fn request_parse_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"session/event","params":{}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "session/event");
        assert!(req.is_notification());
        assert!(req.id.is_none());
    }

    #[test]
    fn request_reject_bad_version() {
        let json = r#"{"jsonrpc":"1.0","method":"test","params":{},"id":1}"#;
        let result: Result<JsonRpcRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn response_success_serialize() {
        let resp =
            JsonRpcResponse::success(Value::Number(1.into()), serde_json::json!({"status": "ok"}));
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert!(parsed.get("result").is_some());
        assert!(parsed.get("error").is_none());
        assert_eq!(parsed["id"], 1);
    }

    #[test]
    fn response_error_serialize() {
        let resp = JsonRpcResponse::error(
            Value::Number(1.into()),
            RpcError::method_not_found("nonexistent"),
        );
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("error").is_some());
        assert_eq!(parsed["error"]["code"], -32601);
        assert!(parsed.get("result").is_none());
    }

    #[test]
    fn notification_serialize() {
        let notif = JsonRpcNotification::new(
            "session/event",
            serde_json::json!({"type": "chunk", "content": "hello"}),
        );
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains(r#""method":"session/event""#));
        assert!(json.contains(r#""content":"hello""#));
    }

    #[test]
    fn error_with_data() {
        let err = RpcError::internal("oops").with_data(serde_json::json!({"retry": true}));
        let json = serde_json::to_string(&err).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["code"], -32603);
        assert_eq!(parsed["message"], "oops");
        assert_eq!(parsed["data"]["retry"], true);
    }
}
