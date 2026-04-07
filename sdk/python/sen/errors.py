"""Error types for the SenWeaverCoding Python SDK."""

from __future__ import annotations

from typing import Any, Optional


class RpcError(Exception):
    """Base exception for all RPC errors."""

    def __init__(self, message: str, code: Optional[int] = None, data: Any = None):
        super().__init__(message)
        self.code = code
        self.data = data


class SessionNotFound(RpcError):
    """Session does not exist or has expired."""


class ToolNotFound(RpcError):
    """The requested tool is not available."""


class InvalidParams(RpcError):
    """Invalid parameters were provided to the RPC method."""


class InternalError(RpcError):
    """Internal error in the SenWeaverCoding runtime."""


class TransportError(RpcError):
    """Failed to connect or communicate over the transport."""


class ParseError(RpcError):
    """The server could not parse the JSON-RPC request."""


# Map JSON-RPC error codes to Python exception types.
_ERROR_CODE_MAP = {
    -32700: ParseError,
    -32602: InvalidParams,
    -32603: InternalError,
    -32000: InternalError,
    -32001: SessionNotFound,
    -32002: ToolNotFound,
}


def from_code(code: int, message: str, data: Any = None) -> RpcError:
    """Construct the appropriate exception type from a JSON-RPC error code."""
    cls = _ERROR_CODE_MAP.get(code, RpcError)
    return cls(message, code=code, data=data)
