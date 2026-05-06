"""Tests for NDJSON message models and transport."""

import json
import pytest

from sen.ndjson_models import (
    ACTION_CAN_USE_TOOL,
    ACTION_MCP_SET_SERVERS,
    ACTION_SESSION_STATE_CHANGED,
    AssistantMessage,
    ControlRequest,
    ControlResponse,
    Result,
    SdkMessage,
    SessionState,
    SystemMessage,
    ToolResult,
    ToolUse,
    UserMessage,
    parse_stdout_message,
)


class TestStdinModels:
    def test_user_message_to_dict(self) -> None:
        msg = UserMessage(content="hello")
        d = msg.to_dict()
        assert d == {"type": "user_message", "content": "hello"}

    def test_user_message_with_images(self) -> None:
        msg = UserMessage(content="look", images=["data:image/png;base64,abc"])
        d = msg.to_dict()
        assert d["images"] == ["data:image/png;base64,abc"]

    def test_control_response_allow(self) -> None:
        msg = ControlResponse(request_id="r1", decision="allow")
        d = msg.to_dict()
        assert d == {"type": "control_response", "request_id": "r1", "decision": "allow"}
        # reason must NOT appear when decision=allow (Rust #[serde(flatten)] rejects it)
        assert "reason" not in d
        assert "updated_input" not in d

    def test_control_response_allow_does_not_include_reason(self) -> None:
        # Even if reason is set, it must not appear when decision=allow
        msg = ControlResponse(request_id="r1", decision="allow", reason="should not appear")
        d = msg.to_dict()
        assert d["decision"] == "allow"
        assert "reason" not in d

    def test_control_response_deny_with_reason(self) -> None:
        msg = ControlResponse(request_id="r2", decision="deny", reason="not safe")
        d = msg.to_dict()
        assert d["decision"] == "deny"
        assert d["reason"] == "not safe"
        # updated_input must NOT appear when decision=deny
        assert "updated_input" not in d

    def test_control_response_deny_does_not_include_updated_input(self) -> None:
        msg = ControlResponse(
            request_id="r2",
            decision="deny",
            updated_input={"should_not_appear": True},
        )
        d = msg.to_dict()
        assert d["decision"] == "deny"
        assert "updated_input" not in d

    def test_control_response_allow_with_updated_input(self) -> None:
        msg = ControlResponse(
            request_id="r3", decision="allow", updated_input={"path": "/safe"}
        )
        d = msg.to_dict()
        assert d["updated_input"] == {"path": "/safe"}
        assert "reason" not in d

    def test_sdk_message_to_dict(self) -> None:
        msg = SdkMessage(action="abort")
        d = msg.to_dict()
        assert d == {"type": "sdk_message", "action": "abort"}

    def test_sdk_message_with_data(self) -> None:
        msg = SdkMessage(action="config", data={"key": "val"})
        d = msg.to_dict()
        assert d["data"] == {"key": "val"}

    def test_roundtrip_serialization(self) -> None:
        msg = UserMessage(content="test")
        json_str = json.dumps(msg.to_dict())
        parsed = json.loads(json_str)
        assert parsed["type"] == "user_message"
        assert parsed["content"] == "test"


class TestStdoutModels:
    def test_assistant_message(self) -> None:
        msg = AssistantMessage(content="hi", stop_reason="end_turn")
        d = msg.to_dict()
        assert d["type"] == "assistant_message"
        assert d["stop_reason"] == "end_turn"

    def test_tool_use(self) -> None:
        msg = ToolUse(tool_use_id="t1", tool_name="bash", input={"command": "ls"})
        d = msg.to_dict()
        assert d["type"] == "tool_use"
        assert d["tool_name"] == "bash"

    def test_tool_result_success(self) -> None:
        msg = ToolResult(tool_use_id="t1", success=True, output="file.txt")
        d = msg.to_dict()
        assert d["success"] is True
        assert "error" not in d

    def test_tool_result_failure(self) -> None:
        msg = ToolResult(tool_use_id="t1", success=False, output="", error="not found")
        d = msg.to_dict()
        assert d["success"] is False
        assert d["error"] == "not found"

    def test_control_request(self) -> None:
        msg = ControlRequest(
            request_id="req1",
            action="can_use_tool",
            tool_name="file_write",
            input={"path": "test.txt"},
        )
        d = msg.to_dict()
        assert d["type"] == "control_request"
        assert d["action"] == "can_use_tool"
        assert d["tool_name"] == "file_write"

    def test_control_request_can_use_tool_excludes_other_fields(self) -> None:
        # When action=can_use_tool, session_id/status/servers must not appear.
        msg = ControlRequest(
            request_id="req1",
            action=ACTION_CAN_USE_TOOL,
            tool_name="bash",
            input={"command": "ls"},
            session_id="s1",  # should NOT appear in output
            status="running",  # should NOT appear
            servers=[],       # should NOT appear
        )
        d = msg.to_dict()
        assert d["action"] == ACTION_CAN_USE_TOOL
        assert "tool_name" in d
        assert "input" in d
        assert "session_id" not in d
        assert "status" not in d
        assert "servers" not in d

    def test_control_request_session_state_changed_excludes_other_fields(self) -> None:
        msg = ControlRequest(
            request_id="req2",
            action=ACTION_SESSION_STATE_CHANGED,
            session_id="s1",
            status="paused",
            tool_name="bash",  # should NOT appear
            servers=[],        # should NOT appear
        )
        d = msg.to_dict()
        assert d["action"] == ACTION_SESSION_STATE_CHANGED
        assert d["session_id"] == "s1"
        assert d["status"] == "paused"
        assert "tool_name" not in d
        assert "servers" not in d

    def test_control_request_mcp_set_servers(self) -> None:
        msg = ControlRequest(
            request_id="req3",
            action=ACTION_MCP_SET_SERVERS,
            servers=[{"name": "filesystem", "type": "stdio"}],
        )
        d = msg.to_dict()
        assert d["action"] == ACTION_MCP_SET_SERVERS
        assert d["servers"] == [{"name": "filesystem", "type": "stdio"}]
        assert "tool_name" not in d
        assert "session_id" not in d

    def test_control_request_factory_can_use_tool(self) -> None:
        msg = ControlRequest.can_use_tool(
            request_id="factory-1",
            tool_name="bash",
            tool_input={"command": "pwd"},
            tool_use_id="tu-42",
        )
        assert msg.request_id == "factory-1"
        assert msg.action == ACTION_CAN_USE_TOOL
        assert msg.tool_name == "bash"
        assert msg.input == {"command": "pwd"}
        assert msg.tool_use_id == "tu-42"

    def test_session_state(self) -> None:
        msg = SessionState(session_id="s1", status="started", metadata={"model": "gpt4"})
        d = msg.to_dict()
        assert d["session_id"] == "s1"
        assert d["metadata"] == {"model": "gpt4"}

    def test_system_message(self) -> None:
        msg = SystemMessage(content="Session started")
        d = msg.to_dict()
        assert d == {"type": "system", "content": "Session started"}

    def test_result_minimal(self) -> None:
        msg = Result()
        d = msg.to_dict()
        assert d == {"type": "result"}

    def test_result_full(self) -> None:
        msg = Result(session_id="s1", cost=0.05, duration_ms=1234, num_turns=3)
        d = msg.to_dict()
        assert d["cost"] == 0.05
        assert d["num_turns"] == 3


class TestParseStdoutMessage:
    def test_parse_assistant_message(self) -> None:
        data = {"type": "assistant_message", "content": "hello"}
        msg = parse_stdout_message(data)
        assert isinstance(msg, AssistantMessage)
        assert msg.content == "hello"

    def test_parse_tool_use(self) -> None:
        data = {
            "type": "tool_use",
            "tool_use_id": "t1",
            "tool_name": "bash",
            "input": {"command": "ls"},
        }
        msg = parse_stdout_message(data)
        assert isinstance(msg, ToolUse)
        assert msg.tool_name == "bash"

    def test_parse_tool_result(self) -> None:
        data = {"type": "tool_result", "tool_use_id": "t1", "success": True, "output": "ok"}
        msg = parse_stdout_message(data)
        assert isinstance(msg, ToolResult)
        assert msg.success is True

    def test_parse_control_request(self) -> None:
        data = {
            "type": "control_request",
            "request_id": "r1",
            "action": "can_use_tool",
            "tool_name": "shell",
        }
        msg = parse_stdout_message(data)
        assert isinstance(msg, ControlRequest)
        assert msg.action == "can_use_tool"

    def test_parse_session_state(self) -> None:
        data = {"type": "session_state", "session_id": "s1", "status": "started"}
        msg = parse_stdout_message(data)
        assert isinstance(msg, SessionState)

    def test_parse_system(self) -> None:
        data = {"type": "system", "content": "debug info"}
        msg = parse_stdout_message(data)
        assert isinstance(msg, SystemMessage)

    def test_parse_result(self) -> None:
        data = {"type": "result", "num_turns": 5}
        msg = parse_stdout_message(data)
        assert isinstance(msg, Result)
        assert msg.num_turns == 5

    def test_parse_unknown_type_raises(self) -> None:
        with pytest.raises(ValueError, match="Unknown stdout"):
            parse_stdout_message({"type": "bogus"})
