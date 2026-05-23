// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

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

pub const PARSE_ERROR: i32 = -32700;

pub const INVALID_REQUEST: i32 = -32600;

pub const METHOD_NOT_FOUND: i32 = -32601;

pub const INVALID_PARAMS: i32 = -32602;

pub const INTERNAL_ERROR: i32 = -32603;

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

    pub const SESSION_NOT_FOUND: i32 = -32000;

    pub const SESSION_LIMIT_REACHED: i32 = -32001;

    pub const SESSION_TIMEOUT: i32 = -32002;

    pub const AGENT_ERROR: i32 = -32003;

    pub const TOOL_ERROR: i32 = -32004;

    pub const MEMORY_ERROR: i32 = -32005;

    pub const BLACKBOARD_ERROR: i32 = -32006;

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
