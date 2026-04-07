// SPDX-License-Identifier: MIT
// SenWeaverCoding TypeScript SDK — main client (JSON-RPC transport).

import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { randomUUID } from "node:crypto";
import type {
  HealthInfo,
  SystemInfo,
  SessionInfo,
  RpcRequest,
  RpcResponse,
  RpcError as RpcErrorPayload,
} from "./types.js";

// ── Errors ──────────────────────────────────────────────────────────

export class RpcError extends Error {
  constructor(
    message: string,
    public readonly code: number,
    public readonly data?: unknown,
  ) {
    super(message);
    this.name = "RpcError";
  }
}

export class TransportError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TransportError";
  }
}

// ── Transport interface ─────────────────────────────────────────────

interface Transport {
  call(method: string, params?: Record<string, unknown>): Promise<unknown>;
  close(): Promise<void>;
}

// ── HttpTransport ───────────────────────────────────────────────────

class HttpTransport implements Transport {
  private readonly url: string;
  private readonly headers: Record<string, string>;
  private readonly timeout: number;

  constructor(url: string, auth?: string, timeout = 120_000) {
    this.url = url.replace(/\/+$/, "") + "/rpc";
    this.headers = { "Content-Type": "application/json" };
    if (auth) this.headers["Authorization"] = `Bearer ${auth}`;
    this.timeout = timeout;
  }

  async call(
    method: string,
    params?: Record<string, unknown>,
  ): Promise<unknown> {
    const body: RpcRequest = {
      jsonrpc: "2.0",
      method,
      id: randomUUID(),
    };
    if (params) body.params = params;

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeout);

    let res: Response;
    try {
      res = await fetch(this.url, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify(body),
        signal: controller.signal,
      });
    } catch (err) {
      throw new TransportError(
        `HTTP request failed: ${err instanceof Error ? err.message : err}`,
      );
    } finally {
      clearTimeout(timer);
    }

    if (!res.ok) {
      throw new TransportError(`HTTP ${res.status}: ${res.statusText}`);
    }

    const data = (await res.json()) as RpcResponse;
    if (data.error) {
      throw new RpcError(
        data.error.message,
        data.error.code,
        data.error.data,
      );
    }
    return data.result;
  }

  async close(): Promise<void> {
    /* HTTP is stateless */
  }
}

// ── StdioTransport ──────────────────────────────────────────────────

class StdioTransport implements Transport {
  private proc: ChildProcess | null = null;
  private pending = new Map<
    string,
    {
      resolve: (v: unknown) => void;
      reject: (e: Error) => void;
    }
  >();

  constructor(private readonly binary: string) {}

  start(): void {
    this.proc = spawn(this.binary, ["rpc", "--stdio"], {
      stdio: ["pipe", "pipe", "pipe"],
    });

    const rl = createInterface({ input: this.proc.stdout! });
    rl.on("line", (line) => {
      if (!line.trim()) return;
      let data: RpcResponse;
      try {
        data = JSON.parse(line);
      } catch {
        return;
      }
      const id = String(data.id);
      const pending = this.pending.get(id);
      if (!pending) return;
      this.pending.delete(id);
      if (data.error) {
        pending.reject(
          new RpcError(data.error.message, data.error.code, data.error.data),
        );
      } else {
        pending.resolve(data.result);
      }
    });

    this.proc.on("error", (err) => {
      for (const [, p] of this.pending) {
        p.reject(new TransportError(`Subprocess error: ${err.message}`));
      }
      this.pending.clear();
    });

    this.proc.on("close", () => {
      for (const [, p] of this.pending) {
        p.reject(new TransportError("Subprocess exited"));
      }
      this.pending.clear();
    });
  }

  async call(
    method: string,
    params?: Record<string, unknown>,
  ): Promise<unknown> {
    if (!this.proc?.stdin?.writable) {
      throw new TransportError("Subprocess not started");
    }

    const id = randomUUID();
    const body: RpcRequest = { jsonrpc: "2.0", method, id };
    if (params) body.params = params;

    return new Promise<unknown>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.proc!.stdin!.write(JSON.stringify(body) + "\n", (err) => {
        if (err) {
          this.pending.delete(id);
          reject(new TransportError(`Write failed: ${err.message}`));
        }
      });
    });
  }

  async close(): Promise<void> {
    if (!this.proc) return;
    this.proc.kill("SIGTERM");
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        this.proc?.kill("SIGKILL");
        resolve();
      }, 5_000);
      this.proc!.on("close", () => {
        clearTimeout(timer);
        resolve();
      });
    });
    this.proc = null;
  }
}

// ── SenAgent client ─────────────────────────────────────────────────

export interface SenAgentOptions {
  httpUrl?: string;
  transport?: "stdio";
  senBinary?: string;
  auth?: string;
  timeout?: number;
}

export class SenAgent {
  private readonly transport: Transport;

  constructor(options: SenAgentOptions) {
    if (options.httpUrl) {
      this.transport = new HttpTransport(
        options.httpUrl,
        options.auth,
        options.timeout,
      );
    } else if (options.transport === "stdio") {
      const binary =
        options.senBinary ??
        process.env["SEN_BIN"] ??
        "sen";
      const stdio = new StdioTransport(binary);
      stdio.start();
      this.transport = stdio;
    } else {
      throw new Error(
        'Must provide either httpUrl or transport: "stdio"',
      );
    }
  }

  async health(): Promise<HealthInfo> {
    const raw = (await this.transport.call("system.health")) as Record<
      string,
      unknown
    >;
    return {
      version: String(raw["version"] ?? "unknown"),
      uptime_secs: Number(raw["uptimeSecs"] ?? 0),
      memory_mb: Number(raw["memoryMb"] ?? 0),
      active_sessions: Number(raw["activeSessions"] ?? 0),
      status: String(raw["status"] ?? "unknown"),
    };
  }

  async systemInfo(): Promise<SystemInfo> {
    const raw = (await this.transport.call("system.info")) as Record<
      string,
      unknown
    >;
    return {
      version: String(raw["version"] ?? "unknown"),
      session_timeout_secs: Number(raw["sessionTimeoutSecs"] ?? 300),
      max_sessions: Number(raw["maxSessions"] ?? 100),
      active_sessions: Number(raw["activeSessions"] ?? 0),
      enabled_transports: (raw["enabledTransports"] as string[]) ?? [],
      workspace_dir: String(raw["workspaceDir"] ?? ""),
    };
  }

  async createSession(opts?: {
    workspaceDir?: string;
    systemPrompt?: string;
  }): Promise<Session> {
    const params: Record<string, unknown> = {};
    if (opts?.workspaceDir) params["workspaceDir"] = opts.workspaceDir;
    if (opts?.systemPrompt) params["systemPrompt"] = opts.systemPrompt;
    const raw = (await this.transport.call("session.new", params)) as Record<
      string,
      unknown
    >;
    return new Session(this.transport, String(raw["sessionId"]));
  }

  async listSessions(): Promise<SessionInfo[]> {
    const raw = (await this.transport.call("session.list")) as Record<
      string,
      unknown
    >;
    const sessions = (raw["sessions"] as Array<Record<string, unknown>>) ?? [];
    return sessions.map((s) => ({
      id: String(s["sessionId"]),
      created_at: String(s["createdAt"]),
      last_active: String(s["lastActive"]),
      workspace_dir: String(s["workspaceDir"] ?? ""),
    }));
  }

  async close(): Promise<void> {
    await this.transport.close();
  }
}

// ── Session ─────────────────────────────────────────────────────────

export class Session {
  constructor(
    private readonly transport: Transport,
    public readonly id: string,
  ) {}

  async prompt(message: string, timeout = 300_000): Promise<string> {
    const result = (await Promise.race([
      this.transport.call("session.prompt", {
        sessionId: this.id,
        message,
      }),
      new Promise<never>((_, reject) =>
        setTimeout(
          () => reject(new Error(`Agent did not respond within ${timeout}ms`)),
          timeout,
        ),
      ),
    ])) as Record<string, unknown>;
    return String(result["response"] ?? "");
  }

  async stop(): Promise<void> {
    await this.transport.call("session.stop", { sessionId: this.id });
  }

  async kill(): Promise<void> {
    await this.transport.call("session.kill", { sessionId: this.id });
  }

  async executeTool(
    name: string,
    args?: Record<string, unknown>,
  ): Promise<unknown> {
    return this.transport.call("tool.exec", {
      sessionId: this.id,
      tool: name,
      args: args ?? {},
    });
  }

  // ── Memory ────────────────────────────────────────────────────────

  async memoryStore(
    content: string,
    opts?: { category?: string; importance?: number; tags?: string[] },
  ): Promise<string> {
    const params: Record<string, unknown> = {
      content,
      category: opts?.category ?? "experience",
    };
    if (opts?.importance !== undefined) params["importance"] = opts.importance;
    if (opts?.tags) params["tags"] = opts.tags;
    const raw = (await this.transport.call("memory.store", params)) as Record<string, unknown>;
    return String(raw["id"] ?? "");
  }

  async memoryRecall(
    query: string,
    opts?: { limit?: number; category?: string },
  ): Promise<Array<Record<string, unknown>>> {
    const params: Record<string, unknown> = { query, limit: opts?.limit ?? 5 };
    if (opts?.category) params["category"] = opts.category;
    const raw = (await this.transport.call("memory.recall", params)) as Record<string, unknown>;
    return (raw["memories"] as Array<Record<string, unknown>>) ?? [];
  }

  // ── Blackboard ──────────────────────────────────────────────────

  async blackboardPut(
    key: string,
    value: unknown,
    opts?: { namespace?: string },
  ): Promise<void> {
    await this.transport.call("blackboard.put", {
      key,
      value,
      namespace: opts?.namespace ?? "default",
    });
  }

  async blackboardGet(
    key: string,
    opts?: { namespace?: string },
  ): Promise<unknown> {
    const raw = (await this.transport.call("blackboard.get", {
      key,
      namespace: opts?.namespace ?? "default",
    })) as Record<string, unknown>;
    return raw["value"];
  }

  async blackboardList(opts?: { namespace?: string }): Promise<string[]> {
    const raw = (await this.transport.call("blackboard.list", {
      namespace: opts?.namespace ?? "default",
    })) as Record<string, unknown>;
    return (raw["keys"] as string[]) ?? [];
  }
}
