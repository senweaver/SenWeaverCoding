# SenWeaverCoding TypeScript SDK

Call AI agents as OS-level services from TypeScript / Node.js.

## Installation

```bash
npm install @senweaver/sen
```

## Quick Start

### HTTP mode (connect to a running agent server)

```typescript
import { SenAgent } from "@senweaver/sen";

const client = new SenAgent({ httpUrl: "http://localhost:42618" });

const health = await client.health();
console.log(`Status: ${health.status}, sessions: ${health.active_sessions}`);

const session = await client.createSession();
const reply = await session.prompt("What can you help me with?");
console.log(reply);

await session.kill();
await client.close();
```

### Stdio mode (spawn a subprocess)

```typescript
import { SenAgent } from "@senweaver/sen";

const client = new SenAgent({ transport: "stdio" });

const session = await client.createSession();
const reply = await session.prompt("Hello, agent!");
console.log(reply);

await session.kill();
await client.close();
```

## NDJSON Headless Mode

For low-level, streaming interaction with the agent's NDJSON protocol:

```typescript
import { NdjsonTransport } from "@senweaver/sen";

const transport = new NdjsonTransport({
  onPermission: async (req) => {
    // req is a discriminated union — narrow by action to access relevant fields.
    if (req.action === "can_use_tool") {
      console.log(`Tool "${req.tool_name}" wants to run — allowing`);
    }
    return true;
  },
});

await transport.start();
await transport.send({ type: "user_message", content: "List files in ." });

for await (const msg of transport.recv()) {
  if (msg.type === "assistant_message") {
    console.log(msg.content);
    break;
  }
}

await transport.close();
```

## API Reference

### `SenAgent`

| Method | Description |
|---|---|
| `health()` | System health info |
| `systemInfo()` | Full system metadata |
| `createSession(opts?)` | Create a new agent session |
| `listSessions()` | List active sessions |
| `close()` | Close the client |

### `Session`

| Method | Description |
|---|---|
| `prompt(message)` | Send a message, get a response |
| `stop()` | Stop the current turn |
| `kill()` | Terminate the session |
| `executeTool(name, args?)` | Execute a tool directly |
| `memoryStore(content, opts?)` | Store a memory entry |
| `memoryRecall(query, opts?)` | Recall memories matching a query |
| `blackboardPut(key, value, opts?)` | Write a value to the shared blackboard |
| `blackboardGet(key, opts?)` | Read a value from the blackboard |
| `blackboardList(opts?)` | List all keys in a namespace |

### `NdjsonTransport`

| Method | Description |
|---|---|
| `start()` | Spawn the subprocess |
| `send(msg)` | Write an NDJSON line to stdin |
| `recv()` | Async generator yielding stdout messages |
| `close()` | Terminate the subprocess |

## See Also

- [Python SDK](../python/README.md) for the equivalent Python client.

## License

MIT
