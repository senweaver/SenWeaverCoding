# SenWeaverCoding Python SDK

Call AI agents as OS-level services from Python.

## Features

- **Multi-transport**: HTTP (network), Stdio (subprocess), or embedded (daemon)
- **Full JSON-RPC 2.0** coverage: sessions, tools, memory, blackboard, system
- **Async-first** API built on `anyio` and `httpx`
- **Streaming support** via callbacks
- **Strongly-typed** models and error types

## Installation

```bash
pip install sen
```

## Quick Start

```python
import asyncio
from sen import SenAgent

async def main():
    async with SenAgent(http_url="http://localhost:42618") as client:
        # Check system health
        health = await client.health()
        print(f"Status: {health.status}, Active sessions: {health.active_sessions}")

        # Create a session
        session = await client.create_session()

        # Chat with the agent
        response = await session.prompt("What can you help me with?")
        print(response)

        # Store a memory
        await session.memory_store("User prefers dark mode", category="preference")

        # Use the shared blackboard
        await session.blackboard_put("user_theme", "dark", namespace="ui")

        # Kill session when done
        await session.kill()

asyncio.run(main())
```

## Transport Modes

### HTTP (default)

```python
async with SenAgent(http_url="http://localhost:42618") as client:
    ...
```

### Stdio (subprocess)

```python
async with SenAgent(transport="stdio") as client:
    ...
```

## Stdio transport

When using `transport="stdio"`, the SDK spawns a `sen rpc --stdio` subprocess and communicates over its stdin/stdout. This is ideal for:

- IDE integration
- CLI tooling
- Isolated, short-lived agent invocations

## Error handling

```python
from sen.errors import SessionNotFound, ToolNotFound, RpcError, TransportError

async def main():
    try:
        session = await client.create_session()
        response = await session.prompt("Hello!")
    except SessionNotFound as e:
        print(f"Session expired: {e}")
    except ToolNotFound as e:
        print(f"Tool not available: {e}")
    except RpcError as e:
        print(f"RPC error {e.code}: {e}")
    except TransportError as e:
        print(f"Connection failed: {e}")
```

## Configuration

### HTTP timeout

```python
async with SenAgent(http_url="http://localhost:42618", timeout=300.0) as client:
    # 5-minute timeout for long-running agent tasks
    response = await session.prompt("Analyze this large codebase...")
```

### Stdio subprocess args

```python
# Point stdio subprocess at a specific config
async with SenAgent(
    transport="stdio",
    # (subprocess args are configured via environment variables or config file)
) as client:
    ...
```

## NDJSON Streaming Mode

For real-time streaming with permission callbacks (the same protocol Claude Code uses for IDE integration):

```python
import asyncio
from sen.transport.ndjson import NdjsonTransport
from sen.ndjson_models import ControlRequest, ControlResponse, UserMessage

async def handle_permission(request: ControlRequest) -> ControlResponse:
    print(f"  [permission] Tool: {request.tool_name} — allowing")
    return ControlResponse(request_id=request.request_id, decision="allow")

async def main():
    async with NdjsonTransport(on_permission=handle_permission) as transport:
        await transport.send(UserMessage(content="List files in the current directory"))

        while True:
            msg = await transport.recv()
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

asyncio.run(main())
```

The `NdjsonTransport` speaks the NDJSON protocol (`sen agent --output-format stream-json`) and handles permission requests by calling the `on_permission` callback. Exceptions in the callback default to **denying** the request for safety.

## Requirements

- Python 3.9+
- `httpx >= 0.25`
- `anyio >= 4.0`

## License

MIT
