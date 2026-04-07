"""SenWeaverCoding Python SDK — call AI agents as OS-level services."""

from __future__ import annotations

import asyncio
import json
import os
import subprocess
from typing import Any, Dict, List, Optional

import anyio

from sen.errors import TransportError, from_code
from sen.models import HealthInfo, SessionInfo, SystemInfo
from sen.transport.base import Transport
from sen.transport.http import HttpTransport


class SenAgent:
    """Python SDK client for SenWeaverCoding.

    Provides an ergonomic high-level API over the JSON-RPC 2.0 interface.
    Supports three transport modes:

    - **HTTP** (default): Connect to a running SenWeaverCoding RPC server over HTTP.
      Use ``SenAgent(http_url="http://localhost:42618")``.
    - **Stdio**: Spawn a SenWeaverCoding subprocess and communicate over stdin/stdout.
      Use ``SenAgent(transport="stdio")``.
    - **Embedded**: Connect to a daemon that embeds the RPC server. Use
      ``SenAgent(embedded=True)`` to use the gateway's internal RPC bridge.

    Example — HTTP::

        async with SenAgent(http_url="http://localhost:42618") as client:
            session = await client.create_session()
            response = await session.prompt("Hello, agent!")
            print(response)

    Example — Stdio::

        async with SenAgent(transport="stdio") as client:
            session = await client.create_session()
            response = await session.prompt("What is 2+2?")
            print(response)
    """

    def __init__(
        self,
        http_url: Optional[str] = None,
        transport: Optional[str] = None,
        embedded: bool = False,
        auth: Optional[str] = None,
        timeout: float = 120.0,
        sen_binary: Optional[str] = None,
    ) -> None:
        self._transport: Optional[Transport] = None
        self._owned_process: Optional[subprocess.Popen] = None
        self._owns_transport = False

        if embedded:
            # TODO: connect to gateway's internal RPC bridge (future enhancement)
            raise NotImplementedError("embedded mode is not yet implemented")

        if http_url:
            self._transport = HttpTransport(http_url, auth=auth, timeout=timeout)
            self._owns_transport = True
        elif transport == "stdio":
            binary = sen_binary or os.environ.get("SEN_BIN", "sen")
            self._owned_process = subprocess.Popen(
                [binary, "rpc", "--stdio"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            stdio_transport = _StdioTransport()
            stdio_transport._start(self._owned_process)
            self._transport = stdio_transport
            self._owns_transport = True
        else:
            raise ValueError(
                "Must provide either http_url=, transport='stdio', or embedded=True"
            )

    async def __aenter__(self) -> "SenAgent":
        return self

    async def __aexit__(self, *_: Any) -> None:
        await self.close()

    async def close(self) -> None:
        if self._owned_process:
            self._owned_process.terminate()
            try:
                self._owned_process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._owned_process.kill()
            self._owned_process = None
        if self._transport and self._owns_transport:
            await self._transport.close()
            self._transport = None

    # ── System ────────────────────────────────────────────────────────

    async def health(self) -> HealthInfo:
        """Return system health information."""
        result = await self._call("system.health")
        return HealthInfo(
            version=result.get("version", "unknown"),
            uptime_secs=float(result.get("uptimeSecs", 0)),
            memory_mb=float(result.get("memoryMb", 0)),
            active_sessions=int(result.get("activeSessions", 0)),
            status=result.get("status", "unknown"),
        )

    async def system_info(self) -> SystemInfo:
        """Return full system information."""
        result = await self._call("system.info")
        return SystemInfo(
            version=result.get("version", "unknown"),
            session_timeout_secs=int(result.get("sessionTimeoutSecs", 300)),
            max_sessions=int(result.get("maxSessions", 100)),
            active_sessions=int(result.get("activeSessions", 0)),
            enabled_transports=result.get("enabledTransports", []),
            workspace_dir=result.get("workspaceDir", ""),
        )

    # ── Sessions ──────────────────────────────────────────────────────

    async def create_session(
        self,
        workspace_dir: Optional[str] = None,
        system_prompt: Optional[str] = None,
    ) -> Session:
        """Create a new agent session.

        Returns a :class:`Session` object. Use :meth:`Session.prompt` to interact.
        """
        params: Dict[str, Any] = {}
        if workspace_dir:
            params["workspaceDir"] = workspace_dir
        if system_prompt:
            params["systemPrompt"] = system_prompt
        result = await self._call("session.new", params)
        return Session(self._transport, result["sessionId"], workspace_dir or "")

    async def list_sessions(self) -> List[SessionInfo]:
        """List all active sessions."""
        result = await self._call("session.list")
        return [
            SessionInfo(
                id=s["sessionId"],
                created_at=s["createdAt"],
                last_active=s["lastActive"],
                workspace_dir=s.get("workspaceDir", ""),
            )
            for s in result.get("sessions", [])
        ]

    # ── Internal ──────────────────────────────────────────────────────

    async def _call(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        assert self._transport is not None, "Client is closed"
        return await self._transport.call(method, params)


class Session:
    """An isolated agent session, created via :meth:`SenAgent.create_session`."""

    def __init__(self, transport: Transport, session_id: str, workspace_dir: str) -> None:
        self._transport = transport
        self._session_id = session_id
        self._workspace_dir = workspace_dir

    @property
    def id(self) -> str:
        """Unique session identifier."""
        return self._session_id

    @property
    def workspace_dir(self) -> str:
        """Workspace directory for this session."""
        return self._workspace_dir

    async def prompt(self, message: str, timeout: float = 300.0) -> str:
        """Send a message and wait for the agent's response.

        Raises:
            TimeoutError: if no response is received within ``timeout`` seconds.
        """
        try:
            result = await asyncio.wait_for(
                self._transport.call("session.prompt", {
                    "sessionId": self._session_id,
                    "message": message,
                }),
                timeout=timeout,
            )
            return result.get("response", "")
        except asyncio.TimeoutError:
            raise TimeoutError(
                f"Agent did not respond within {timeout}s. "
                "Use session.prompt_stream() for long-running tasks."
            )

    async def prompt_stream(
        self,
        message: str,
        on_chunk: Optional[callable] = None,
        on_thinking: Optional[callable] = None,
        on_tool_call: Optional[callable] = None,
        on_tool_result: Optional[callable] = None,
    ) -> str:
        """Send a message and stream events as they arrive.

        Keyword callbacks are invoked with the event data as it arrives.
        The final assistant response is returned.

        For HTTP transport, this uses the ``session/prompt_stream`` RPC method
        and reads ``session/event`` notification lines from the response.

        For Stdio transport, notifications arrive as JSON-RPC lines with
        ``method: "session/event"`` interleaved with the final response.
        """
        if not isinstance(self._transport, (_StdioTransport, HttpTransport)):
            return await self.prompt(message)

        result = {"response": ""}

        if isinstance(self._transport, _StdioTransport):
            result = await self._stream_via_stdio(
                message, on_chunk, on_thinking, on_tool_call, on_tool_result,
            )
        else:
            result = await self._stream_via_http(
                message, on_chunk, on_thinking, on_tool_call, on_tool_result,
            )

        return result.get("response", "")

    async def _stream_via_stdio(
        self,
        message: str,
        on_chunk: Optional[callable] = None,
        on_thinking: Optional[callable] = None,
        on_tool_call: Optional[callable] = None,
        on_tool_result: Optional[callable] = None,
    ) -> Dict[str, Any]:
        """Stream events via stdio JSON-RPC notifications."""
        transport: _StdioTransport = self._transport  # type: ignore
        if not transport._proc or not transport._proc.stdin or not transport._proc.stdout:
            raise TransportError("Subprocess not started")

        import uuid as _uuid
        req_id = str(_uuid.uuid4())
        payload = {
            "jsonrpc": "2.0",
            "method": "session.prompt_stream",
            "params": {"sessionId": self._session_id, "message": message},
            "id": req_id,
        }
        line = json.dumps(payload) + "\n"
        try:
            transport._proc.stdin.write(line.encode())
            transport._proc.stdin.flush()
        except BrokenPipeError as e:
            raise TransportError(f"Subprocess stdin closed: {e}") from e

        final_result: Dict[str, Any] = {}
        while True:
            try:
                import anyio.to_thread
                raw = await anyio.to_thread.run_sync(transport._proc.stdout.readline)
            except Exception as e:
                raise TransportError(f"Read failed: {e}") from e
            if not raw:
                break
            data = json.loads(raw)
            if data.get("id") == req_id:
                if "error" in data:
                    err = data["error"]
                    raise from_code(err.get("code", -32000), err.get("message", ""))
                final_result = data.get("result", {})
                break
            if data.get("method") == "session/event":
                params = data.get("params", {})
                _dispatch_event(params, on_chunk, on_thinking, on_tool_call, on_tool_result)
        return final_result

    async def _stream_via_http(
        self,
        message: str,
        on_chunk: Optional[callable] = None,
        on_thinking: Optional[callable] = None,
        on_tool_call: Optional[callable] = None,
        on_tool_result: Optional[callable] = None,
    ) -> Dict[str, Any]:
        """Stream events via HTTP SSE-style response."""
        import httpx
        import uuid as _uuid

        transport: HttpTransport = self._transport  # type: ignore
        req_id = str(_uuid.uuid4())
        payload = {
            "jsonrpc": "2.0",
            "method": "session.prompt_stream",
            "params": {"sessionId": self._session_id, "message": message},
            "id": req_id,
        }
        final_result: Dict[str, Any] = {}
        try:
            async with transport._client.stream("POST", "/rpc", json=payload) as resp:
                async for line_bytes in resp.aiter_lines():
                    line_str = line_bytes.strip()
                    if not line_str:
                        continue
                    try:
                        data = json.loads(line_str)
                    except json.JSONDecodeError:
                        continue
                    if data.get("id") == req_id:
                        if "error" in data:
                            err = data["error"]
                            raise from_code(err.get("code", -32000), err.get("message", ""))
                        final_result = data.get("result", {})
                    elif data.get("method") == "session/event":
                        params = data.get("params", {})
                        _dispatch_event(params, on_chunk, on_thinking, on_tool_call, on_tool_result)
        except httpx.ConnectError as e:
            raise TransportError(f"Connection failed: {e}") from e
        except httpx.TimeoutException as e:
            raise TransportError(f"Stream timed out: {e}") from e
        return final_result

    async def stop(self) -> None:
        """Stop the current turn in progress."""
        await self._transport.call("session.stop", {"sessionId": self._session_id})

    async def kill(self) -> None:
        """Terminate the session and release its resources."""
        await self._transport.call("session.kill", {"sessionId": self._session_id})

    async def execute_tool(
        self, name: str, args: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """Execute a tool within this session."""
        return await self._transport.call("tool.exec", {
            "sessionId": self._session_id,
            "tool": name,
            "args": args or {},
        })

    # ── Memory ────────────────────────────────────────────────────────

    async def memory_store(
        self,
        content: str,
        category: str = "experience",
        importance: Optional[float] = None,
        tags: Optional[List[str]] = None,
    ) -> str:
        """Store a memory entry in the agent's memory system."""
        params: Dict[str, Any] = {"content": content, "category": category}
        if importance is not None:
            params["importance"] = importance
        if tags:
            params["tags"] = tags
        result = await self._call("memory.store", params)
        return result.get("id", "")

    async def memory_recall(
        self,
        query: str,
        limit: int = 5,
        category: Optional[str] = None,
    ) -> List[Dict[str, Any]]:
        """Recall memories matching a query."""
        params: Dict[str, Any] = {"query": query, "limit": limit}
        if category:
            params["category"] = category
        result = await self._call("memory.recall", params)
        return result.get("memories", [])

    # ── Blackboard ────────────────────────────────────────────────────

    async def blackboard_put(
        self, key: str, value: Any, namespace: str = "default"
    ) -> None:
        """Write a value to the shared blackboard."""
        await self._call("blackboard.put", {
            "key": key,
            "value": value,
            "namespace": namespace,
        })

    async def blackboard_get(
        self, key: str, namespace: str = "default"
    ) -> Any:
        """Read a value from the shared blackboard."""
        result = await self._call("blackboard.get", {
            "key": key,
            "namespace": namespace,
        })
        return result.get("value")

    async def blackboard_list(self, namespace: str = "default") -> List[str]:
        """List all keys in a namespace."""
        result = await self._call("blackboard.list", {"namespace": namespace})
        return result.get("keys", [])

    async def _call(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        return await self._transport.call(method, params)


def _dispatch_event(
    params: Dict[str, Any],
    on_chunk: Optional[callable] = None,
    on_thinking: Optional[callable] = None,
    on_tool_call: Optional[callable] = None,
    on_tool_result: Optional[callable] = None,
) -> None:
    """Route a streaming event to the appropriate callback."""
    event_type = params.get("type", "")
    if event_type == "chunk" and on_chunk:
        on_chunk(params)
    elif event_type == "thinking" and on_thinking:
        on_thinking(params)
    elif event_type == "tool_call" and on_tool_call:
        on_tool_call(params)
    elif event_type == "tool_result" and on_tool_result:
        on_tool_result(params)


class _StdioTransport(Transport):
    """JSON-RPC 2.0 transport over stdin/stdout subprocess pipe."""

    def __init__(self) -> None:
        self._proc: Optional[subprocess.Popen] = None
        self._reader_task: Optional[anyio.TaskGroup] = None

    def _start(self, proc: subprocess.Popen) -> None:
        self._proc = proc

    async def call(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        if not self._proc or not self._proc.stdin or not self._proc.stdout:
            raise TransportError("Subprocess not started")

        payload = {"jsonrpc": "2.0", "method": method}
        if params:
            payload["params"] = params
        import uuid
        payload["id"] = str(uuid.uuid4())

        line = json.dumps(payload) + "\n"
        try:
            self._proc.stdin.write(line.encode())
            self._proc.stdin.flush()
        except BrokenPipeError as e:
            raise TransportError(f"Subprocess stdin closed: {e}") from e

        # Read response line
        try:
            import anyio.to_thread
            raw = await anyio.to_thread.run_sync(self._proc.stdout.readline)
        except Exception as e:
            raise TransportError(f"Failed to read from subprocess stdout: {e}") from e

        if not raw:
            raise TransportError("Subprocess stdout returned empty response")

        resp = json.loads(raw)
        if "error" in resp:
            err = resp["error"]
            raise from_code(
                err.get("code", -32000),
                err.get("message", "Unknown error"),
                err.get("data"),
            )
        return resp.get("result")

    async def close(self) -> None:
        if self._proc:
            self._proc.terminate()
            try:
                self._proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._proc.kill()
            self._proc = None
