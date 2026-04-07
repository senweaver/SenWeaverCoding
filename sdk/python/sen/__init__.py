"""SenWeaverCoding Python SDK — call AI agents as OS-level services."""

from sen.client import SenAgent, Session
from sen.errors import (
    InternalError,
    InvalidParams,
    ParseError,
    RpcError,
    SessionNotFound,
    ToolNotFound,
    TransportError,
)
from sen.models import (
    BlackboardEntry,
    HealthInfo,
    MemoryEntry,
    SessionInfo,
    SystemInfo,
    ToolExecutionResult,
    ToolSpec,
)
from sen.ndjson_models import (
    AssistantMessage,
    ControlRequest,
    ControlResponse,
    Result,
    SessionState,
    SdkMessage,
    SystemMessage,
    ToolResult,
    ToolUse,
    UserMessage,
    # Action constants for ControlRequest
    ACTION_CAN_USE_TOOL,
    ACTION_SESSION_STATE_CHANGED,
    ACTION_MCP_SET_SERVERS,
)

__all__ = [
    # Client
    "SenAgent",
    "Session",
    # Errors
    "RpcError",
    "SessionNotFound",
    "ToolNotFound",
    "InvalidParams",
    "InternalError",
    "TransportError",
    "ParseError",
    # Models (RPC)
    "SessionInfo",
    "ToolSpec",
    "HealthInfo",
    "SystemInfo",
    "MemoryEntry",
    "BlackboardEntry",
    "ToolExecutionResult",
    # Models (NDJSON)
    "UserMessage",
    "ControlResponse",
    "SdkMessage",
    "AssistantMessage",
    "ToolUse",
    "ToolResult",
    "ControlRequest",
    "SessionState",
    "SystemMessage",
    "Result",
    # Action constants
    "ACTION_CAN_USE_TOOL",
    "ACTION_SESSION_STATE_CHANGED",
    "ACTION_MCP_SET_SERVERS",
]
__version__ = "0.1.0"
