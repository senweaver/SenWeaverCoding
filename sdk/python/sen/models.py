"""Python SDK for SenWeaverCoding RPC."""

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional


@dataclass
class SessionInfo:
    """Information about an active agent session."""

    id: str
    created_at: str
    last_active: str
    workspace_dir: str


@dataclass
class ToolSpec:
    """Specification for an available tool."""

    name: str
    description: str
    parameters: Dict[str, Any]


@dataclass
class HealthInfo:
    """System health information."""

    version: str
    uptime_secs: float
    memory_mb: float
    active_sessions: int
    status: str


@dataclass
class MemoryEntry:
    """A stored memory entry."""

    id: str
    content: str
    category: str
    created_at: str
    importance: Optional[float] = None
    tags: List[str] = field(default_factory=list)


@dataclass
class BlackboardEntry:
    """A blackboard key-value entry."""

    key: str
    value: Any
    namespace: str
    updated_at: str


@dataclass
class ToolExecutionResult:
    """Result of a tool execution."""

    name: str
    output: str
    success: bool


@dataclass
class SystemInfo:
    """Full system information."""

    version: str
    session_timeout_secs: int
    max_sessions: int
    active_sessions: int
    enabled_transports: List[str]
    workspace_dir: str
