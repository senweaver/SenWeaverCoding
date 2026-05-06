"""Tests for the SenWeaverCoding Python SDK."""

import pytest
from unittest.mock import AsyncMock, MagicMock, patch

from sen import SenAgent
from sen.errors import (
    RpcError,
    SessionNotFound,
    ToolNotFound,
    from_code,
    ParseError,
    InvalidParams,
    TransportError,
)
from sen.models import HealthInfo
from sen.transport.http import HttpTransport

class TestErrorTypes:
    def test_from_code_parse_error(self) -> None:
        err = from_code(-32700, "Parse error")
        assert isinstance(err, ParseError)
        assert err.code == -32700

    def test_from_code_invalid_params(self) -> None:
        err = from_code(-32602, "Invalid params")
        assert isinstance(err, InvalidParams)
        assert err.code == -32602

    def test_from_code_session_not_found(self) -> None:
        err = from_code(-32001, "Session not found")
        assert isinstance(err, SessionNotFound)
        assert err.code == -32001

    def test_from_code_tool_not_found(self) -> None:
        err = from_code(-32002, "Tool not found")
        assert isinstance(err, ToolNotFound)
        assert err.code == -32002

    def test_from_code_unknown_returns_rpc_error(self) -> None:
        err = from_code(-32099, "Unknown error")
        assert isinstance(err, RpcError)
        assert not isinstance(err, SessionNotFound)

    def test_rpc_error_carries_data(self) -> None:
        err = from_code(-32603, "Internal error", data={"foo": "bar"})
        assert err.data == {"foo": "bar"}


class TestHttpTransport:
    def test_rejects_non_http_url(self) -> None:
        with pytest.raises(ValueError, match="must start with http"):
            HttpTransport("ftp://localhost")

    @pytest.mark.asyncio
    async def test_call_parses_error_response(self) -> None:
        transport = HttpTransport("http://localhost:42618")
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {
            "jsonrpc": "2.0",
            "id": "1",
            "error": {"code": -32001, "message": "Session not found"},
        }
        transport._client = AsyncMock()
        transport._client.post.return_value = mock_resp

        with pytest.raises(SessionNotFound) as exc_info:
            await transport.call("session.prompt")
        assert exc_info.value.code == -32001

    @pytest.mark.asyncio
    async def test_call_returns_result(self) -> None:
        transport = HttpTransport("http://localhost:42618")
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {"jsonrpc": "2.0", "id": "1", "result": {"ok": True}}
        transport._client = AsyncMock()
        transport._client.post.return_value = mock_resp

        result = await transport.call("system.health")
        assert result == {"ok": True}

    @pytest.mark.asyncio
    async def test_call_handles_connection_error(self) -> None:
        import httpx

        transport = HttpTransport("http://localhost:42618")
        transport._client = AsyncMock()
        transport._client.post.side_effect = httpx.ConnectError("Connection refused")

        with pytest.raises(TransportError, match="Connection failed"):
            await transport.call("system.health")

    @pytest.mark.asyncio
    async def test_call_handles_timeout(self) -> None:
        import httpx

        transport = HttpTransport("http://localhost:42618")
        transport._client = AsyncMock()
        transport._client.post.side_effect = httpx.TimeoutException("timed out")

        with pytest.raises(TransportError, match="timed out"):
            await transport.call("system.health")

    @pytest.mark.asyncio
    async def test_close(self) -> None:
        transport = HttpTransport("http://localhost:42618")
        transport._client = AsyncMock()
        await transport.close()
        transport._client.aclose.assert_called_once()

    @pytest.mark.asyncio
    async def test_context_manager(self) -> None:
        transport = HttpTransport("http://localhost:42618")
        transport._client = AsyncMock()

        async with transport as t:
            assert t is transport

        transport._client.aclose.assert_called_once()


class TestSenAgentHttp:
    @pytest.mark.asyncio
    async def test_health(self) -> None:
        mock_transport = AsyncMock()
        mock_transport.call.return_value = {
            "version": "0.1.0",
            "uptimeSecs": 1234.5,
            "memoryMb": 42.0,
            "activeSessions": 2,
            "status": "healthy",
        }

        # Direct construction with http_url requires a real async context.
        # Use object.__new__ to bypass __init__ and inject a mock transport.
        client = object.__new__(SenAgent)
        client._transport = mock_transport
        client._owns_transport = False

        health = await client.health()
        assert isinstance(health, HealthInfo)
        assert health.version == "0.1.0"
        assert health.uptime_secs == 1234.5
        assert health.active_sessions == 2
        assert health.status == "healthy"

    @pytest.mark.asyncio
    async def test_create_session(self) -> None:
        mock_transport = AsyncMock()
        mock_transport.call.return_value = {
            "sessionId": "abc-123",
            "createdAt": "2026-01-01T00:00:00Z",
            "lastActive": "2026-01-01T00:00:01Z",
            "workspaceDir": "/tmp/sen",
        }

        client = object.__new__(SenAgent)
        client._transport = mock_transport
        client._owns_transport = False

        session = await client.create_session(workspace_dir="/tmp/test")
        assert session.id == "abc-123"
        assert session.workspace_dir == "/tmp/test"
        mock_transport.call.assert_called_once()
        call_args = mock_transport.call.call_args
        assert call_args[0][0] == "session.new"
        assert call_args[0][1]["workspaceDir"] == "/tmp/test"

    @pytest.mark.asyncio
    async def test_list_sessions(self) -> None:
        mock_transport = AsyncMock()
        mock_transport.call.return_value = {
            "sessions": [
                {
                    "sessionId": "s1",
                    "createdAt": "2026-01-01T00:00:00Z",
                    "lastActive": "2026-01-01T00:00:05Z",
                    "workspaceDir": "/tmp/ws1",
                },
                {
                    "sessionId": "s2",
                    "createdAt": "2026-01-01T00:00:10Z",
                    "lastActive": "2026-01-01T00:00:15Z",
                    "workspaceDir": "/tmp/ws2",
                },
            ]
        }

        client = object.__new__(SenAgent)
        client._transport = mock_transport
        client._owns_transport = False

        sessions = await client.list_sessions()
        assert len(sessions) == 2
        assert sessions[0].id == "s1"
        assert sessions[1].id == "s2"


class TestSession:
    @pytest.mark.asyncio
    async def test_prompt(self) -> None:
        mock_transport = AsyncMock()
        mock_transport.call.return_value = {"response": "42"}

        from sen.client import Session

        session = Session(mock_transport, "test-session", "/tmp/ws")
        result = await session.prompt("What is 2+2?")
        assert result == "42"
        mock_transport.call.assert_called_once_with(
            "session.prompt",
            {"sessionId": "test-session", "message": "What is 2+2?"},
        )

    @pytest.mark.asyncio
    async def test_prompt_timeout(self) -> None:
        import asyncio

        mock_transport = AsyncMock()
        mock_transport.call.side_effect = asyncio.TimeoutError()

        from sen.client import Session

        session = Session(mock_transport, "test-session", "/tmp/ws")
        with pytest.raises(TimeoutError, match="did not respond"):
            await asyncio.wait_for(session.prompt("hello", timeout=0.1), timeout=1.0)

    @pytest.mark.asyncio
    async def test_execute_tool(self) -> None:
        mock_transport = AsyncMock()
        mock_transport.call.return_value = {"name": "bash", "output": "ok", "success": True}

        from sen.client import Session

        session = Session(mock_transport, "test-session", "/tmp/ws")
        result = await session.execute_tool("bash", {"command": "echo ok"})
        assert result["success"] is True

    @pytest.mark.asyncio
    async def test_memory_store_and_recall(self) -> None:
        mock_transport = AsyncMock()
        mock_transport.call.return_value = {"id": "mem-1"}

        from sen.client import Session

        session = Session(mock_transport, "test-session", "/tmp/ws")
        mem_id = await session.memory_store("test memory", category="fact")
        assert mem_id == "mem-1"

        mock_transport.call.return_value = {
            "memories": [
                {"id": "mem-1", "content": "test memory", "category": "fact"}
            ]
        }
        memories = await session.memory_recall("test")
        assert len(memories) == 1
        assert memories[0]["content"] == "test memory"

    @pytest.mark.asyncio
    async def test_blackboard_put_get_list(self) -> None:
        mock_transport = AsyncMock()
        mock_transport.call.side_effect = [
            None,  # put
            {"value": "dark"},  # get — must be a dict so .get("value") works
            {"keys": ["theme", "language"]},  # list
        ]

        from sen.client import Session

        session = Session(mock_transport, "test-session", "/tmp/ws")

        await session.blackboard_put("theme", "dark")
        val = await session.blackboard_get("theme")
        assert val == "dark"
        keys = await session.blackboard_list()
        assert keys == ["theme", "language"]
