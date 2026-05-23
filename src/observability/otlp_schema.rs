// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub const SPAN_AGENT_TURN: &str = "agent.turn";

pub const SPAN_AGENT_TOOL_CALL: &str = "agent.tool_call";

pub const SPAN_PROVIDER_COMPLETE: &str = "provider.complete";

pub const SPAN_MEMORY_SEARCH: &str = "memory.search";

pub const SPAN_GATEWAY_REQUEST: &str = "gateway.request";

pub const SPAN_SESSION_EVENT: &str = "session.event";

pub mod attrs {
    pub const AGENT_ID: &str = "agent.id";
    pub const AGENT_ROLE: &str = "agent.role";
    pub const PROVIDER_NAME: &str = "provider.name";
    pub const PROVIDER_MODEL: &str = "provider.model";
    pub const TOOL_NAME: &str = "tool.name";
    pub const TOOL_SUCCESS: &str = "tool.success";
    pub const TOOL_CACHE_HIT: &str = "tool.cache_hit";
    pub const TURN_INDEX: &str = "turn.index";
    pub const TURN_TOKENS_IN: &str = "turn.tokens_in";
    pub const TURN_TOKENS_OUT: &str = "turn.tokens_out";
    pub const SESSION_ID: &str = "session.id";
    pub const SESSION_EVENT_KIND: &str = "session.event.kind";
    pub const GATEWAY_ROUTE: &str = "gateway.route";
    pub const GATEWAY_STATUS: &str = "gateway.status";
    pub const ERROR_CLASS: &str = "error.class";
}

pub fn is_schema_attr(key: &str) -> bool {
    matches!(
        key,
        attrs::AGENT_ID
            | attrs::AGENT_ROLE
            | attrs::PROVIDER_NAME
            | attrs::PROVIDER_MODEL
            | attrs::TOOL_NAME
            | attrs::TOOL_SUCCESS
            | attrs::TOOL_CACHE_HIT
            | attrs::TURN_INDEX
            | attrs::TURN_TOKENS_IN
            | attrs::TURN_TOKENS_OUT
            | attrs::SESSION_ID
            | attrs::SESSION_EVENT_KIND
            | attrs::GATEWAY_ROUTE
            | attrs::GATEWAY_STATUS
            | attrs::ERROR_CLASS
    )
}

pub const ALL_SPANS: &[&str] = &[
    SPAN_AGENT_TURN,
    SPAN_AGENT_TOOL_CALL,
    SPAN_PROVIDER_COMPLETE,
    SPAN_MEMORY_SEARCH,
    SPAN_GATEWAY_REQUEST,
    SPAN_SESSION_EVENT,
];
