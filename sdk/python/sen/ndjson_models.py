"""Typed dataclasses mirroring the Rust StdinMessage / StdoutMessage NDJSON protocol."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, List, Optional, Union


# ── Stdin messages (SDK → agent) ─────────────────────────────────────


@dataclass
class UserMessage:
    content: str
    images: List[str] = field(default_factory=list)

    def to_dict(self) -> dict:
        d: dict = {"type": "user_message", "content": self.content}
        if self.images:
            d["images"] = self.images
        return d


@dataclass
class ControlResponse:
    request_id: str
    decision: str  # "allow" | "deny"
    updated_input: Any = None
    reason: Optional[str] = None

    def to_dict(self) -> dict:
        d: dict = {
            "type": "control_response",
            "request_id": self.request_id,
            "decision": self.decision,
        }
        # Rust's #[serde(flatten)] on ControlResponsePayload means only the
        # fields matching the active variant should be present.  A "null"
        # explicit value causes serde to emit `"reason": null` which Rust
        # rejects (Option<String> only accepts absent or non-null string).
        if self.decision == "allow":
            if self.updated_input is not None:
                d["updated_input"] = self.updated_input
        elif self.decision == "deny":
            if self.reason is not None:
                d["reason"] = self.reason
        return d


@dataclass
class SdkMessage:
    action: str
    data: Any = None

    def to_dict(self) -> dict:
        d: dict = {"type": "sdk_message", "action": self.action}
        if self.data is not None:
            d["data"] = self.data
        return d


StdinMessage = Union[UserMessage, ControlResponse, SdkMessage]


# ── Stdout messages (agent → SDK) ────────────────────────────────────


@dataclass
class AssistantMessage:
    content: str
    stop_reason: Optional[str] = None

    def to_dict(self) -> dict:
        d: dict = {"type": "assistant_message", "content": self.content}
        if self.stop_reason is not None:
            d["stop_reason"] = self.stop_reason
        return d


@dataclass
class ToolUse:
    tool_use_id: str
    tool_name: str
    input: Any = None

    def to_dict(self) -> dict:
        return {
            "type": "tool_use",
            "tool_use_id": self.tool_use_id,
            "tool_name": self.tool_name,
            "input": self.input,
        }


@dataclass
class ToolResult:
    tool_use_id: str
    success: bool
    output: str
    error: Optional[str] = None

    def to_dict(self) -> dict:
        d: dict = {
            "type": "tool_result",
            "tool_use_id": self.tool_use_id,
            "success": self.success,
            "output": self.output,
        }
        if self.error is not None:
            d["error"] = self.error
        return d


# Valid action strings, used for type narrowing / validation.
ACTION_CAN_USE_TOOL = "can_use_tool"
ACTION_SESSION_STATE_CHANGED = "session_state_changed"
ACTION_MCP_SET_SERVERS = "mcp_set_servers"


@dataclass
class ControlRequest:
    request_id: str
    action: str
    tool_name: Optional[str] = None
    input: Any = None
    tool_use_id: Optional[str] = None
    session_id: Optional[str] = None
    status: Optional[str] = None
    servers: Optional[List] = None

    def to_dict(self) -> dict:
        d: dict = {
            "type": "control_request",
            "request_id": self.request_id,
            "action": self.action,
        }
        # Emit only the fields that belong to the active action variant.
        # Rust's #[serde(tag = "action")] + #[serde(flatten)] on
        # ControlRequestPayload rejects extra fields that are not part of
        # the active variant.
        if self.action == ACTION_CAN_USE_TOOL:
            if self.tool_name is not None:
                d["tool_name"] = self.tool_name
            if self.input is not None:
                d["input"] = self.input
            if self.tool_use_id is not None:
                d["tool_use_id"] = self.tool_use_id
        elif self.action == ACTION_SESSION_STATE_CHANGED:
            if self.session_id is not None:
                d["session_id"] = self.session_id
            if self.status is not None:
                d["status"] = self.status
        elif self.action == ACTION_MCP_SET_SERVERS:
            if self.servers is not None:
                d["servers"] = self.servers
        return d

    @classmethod
    def can_use_tool(
        cls, request_id: str, tool_name: str, tool_input: Any, tool_use_id: Optional[str] = None
    ) -> "ControlRequest":
        """Build a ``can_use_tool`` control request."""
        return cls(
            request_id=request_id,
            action=ACTION_CAN_USE_TOOL,
            tool_name=tool_name,
            input=tool_input,
            tool_use_id=tool_use_id,
        )

    @classmethod
    def session_state_changed(cls, request_id: str, session_id: str, status: str) -> "ControlRequest":
        """Build a ``session_state_changed`` control request."""
        return cls(
            request_id=request_id,
            action=ACTION_SESSION_STATE_CHANGED,
            session_id=session_id,
            status=status,
        )

    @classmethod
    def mcp_set_servers(cls, request_id: str, servers: List) -> "ControlRequest":
        """Build an ``mcp_set_servers`` control request."""
        return cls(request_id=request_id, action=ACTION_MCP_SET_SERVERS, servers=servers)


@dataclass
class SessionState:
    session_id: str
    status: str = ""
    metadata: Any = None

    def to_dict(self) -> dict:
        d: dict = {
            "type": "session_state",
            "session_id": self.session_id,
            "status": self.status,
        }
        if self.metadata is not None:
            d["metadata"] = self.metadata
        return d


@dataclass
class SystemMessage:
    content: str

    def to_dict(self) -> dict:
        return {"type": "system", "content": self.content}


@dataclass
class Result:
    session_id: Optional[str] = None
    cost: Optional[float] = None
    duration_ms: Optional[int] = None
    num_turns: Optional[int] = None

    def to_dict(self) -> dict:
        d: dict = {"type": "result"}
        if self.session_id is not None:
            d["session_id"] = self.session_id
        if self.cost is not None:
            d["cost"] = self.cost
        if self.duration_ms is not None:
            d["duration_ms"] = self.duration_ms
        if self.num_turns is not None:
            d["num_turns"] = self.num_turns
        return d


StdoutMessage = Union[
    AssistantMessage,
    ToolUse,
    ToolResult,
    ControlRequest,
    SessionState,
    SystemMessage,
    Result,
]

_STDOUT_TYPE_MAP: dict[str, type] = {
    "assistant_message": AssistantMessage,
    "tool_use": ToolUse,
    "tool_result": ToolResult,
    "control_request": ControlRequest,
    "session_state": SessionState,
    "system": SystemMessage,
    "result": Result,
}


def parse_stdout_message(data: dict) -> StdoutMessage:
    """Parse a dict (from a JSON line) into the appropriate StdoutMessage dataclass."""
    msg_type = data.get("type")
    cls = _STDOUT_TYPE_MAP.get(msg_type)  # type: ignore[arg-type]
    if cls is None:
        raise ValueError(f"Unknown stdout message type: {msg_type!r}")

    kwargs = {k: v for k, v in data.items() if k != "type"}
    return cls(**kwargs)
