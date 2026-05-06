"""Example: Drive SenWeaverCoding in headless/SDK mode via NDJSON.

This demonstrates the NDJSON structured I/O protocol, which is the same
protocol CC (Claude Code) uses for IDE integration. Unlike the JSON-RPC
transport, this mode streams events in real-time and supports the
permission control protocol.

Usage:
    python examples/headless.py

Requires the ``sen`` binary to be in $PATH or set via $SEN_BIN.
"""

import asyncio

from sen.ndjson_models import ControlRequest, ControlResponse, UserMessage
from sen.transport.ndjson import NdjsonTransport


async def handle_permission(request: ControlRequest) -> ControlResponse:
    """Auto-approve all tool uses for this demo."""
    print(f"  [permission] Tool: {request.tool_name} — auto-allowing")
    return ControlResponse(request_id=request.request_id, decision="allow")


async def main() -> None:
    async with NdjsonTransport(on_permission=handle_permission) as transport:
        await transport.send(UserMessage(content="What files are in the current directory?"))

        while True:
            msg = await asyncio.wait_for(transport.recv(), timeout=120.0)

            type_name = type(msg).__name__
            if hasattr(msg, "content"):
                print(f"[{type_name}] {msg.content}")
            elif hasattr(msg, "tool_name"):
                print(f"[{type_name}] {msg.tool_name}({msg.input})")
            elif hasattr(msg, "output"):
                status = "ok" if msg.success else "FAIL"
                print(f"[{type_name}] [{status}] {msg.output[:200]}")
            elif hasattr(msg, "num_turns"):
                print(f"[{type_name}] Done — {msg.num_turns} turns, cost=${msg.cost}")
                break
            else:
                print(f"[{type_name}] {msg}")


if __name__ == "__main__":
    asyncio.run(main())
