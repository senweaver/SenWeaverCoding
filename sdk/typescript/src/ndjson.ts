// SPDX-License-Identifier: MIT
// NDJSON transport for headless / SDK mode.
// Speaks the streaming NDJSON protocol (--output-format stream-json), NOT JSON-RPC.

import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import type {
  StdinMessage,
  StdoutMessage,
  ControlRequestCanUseTool,
  ControlRequestSessionStateChanged,
  ControlRequestMcpSetServers,
  ControlResponseAllow,
  ControlResponseDeny,
} from "./types.js";

export type PermissionDecision =
  | ControlResponseAllow
  | ControlResponseDeny
  | boolean;

export interface NdjsonOptions {
  binary?: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  onPermission?: (
    req: ControlRequestCanUseTool | ControlRequestSessionStateChanged | ControlRequestMcpSetServers,
  ) => PermissionDecision | Promise<PermissionDecision>;
}

export class NdjsonTransport {
  private proc: ChildProcess | null = null;
  private queue: StdoutMessage[] = [];
  private waiters: Array<(msg: StdoutMessage) => void> = [];
  private closed = false;

  private readonly binary: string;
  private readonly args: string[];
  private readonly cwd?: string;
  private readonly env?: Record<string, string>;
  private readonly onPermission?: NdjsonOptions["onPermission"];

  constructor(options?: NdjsonOptions) {
    this.binary = options?.binary ?? "sen";
    this.args = options?.args ?? [];
    this.cwd = options?.cwd;
    this.env = options?.env;
    this.onPermission = options?.onPermission;
  }

  async start(): Promise<void> {
    const cmd = ["agent", "--output-format", "stream-json", ...this.args];
    this.proc = spawn(this.binary, cmd, {
      // Do NOT merge with process.env when env is empty — that causes the
      // subprocess to lose PATH and other essential variables on Windows.
      stdio: ["pipe", "pipe", "pipe"],
      cwd: this.cwd,
      env: this.env ? { ...process.env, ...this.env } : undefined,
    });

    // Drain stderr to prevent the subprocess from blocking on EPIPE.
    this.proc.stderr?.on("data", (chunk: Buffer) => {
      process.stderr.write(chunk);
    });

    const rl = createInterface({ input: this.proc.stdout! });
    rl.on("line", (line) => {
      const trimmed = line.trim();
      if (!trimmed) return;

      let data: Record<string, unknown>;
      try {
        data = JSON.parse(trimmed);
      } catch {
        return;
      }

      const msg = data as unknown as StdoutMessage;
      if (msg.type === "control_request" && this.onPermission) {
        void this.handleControlRequest(msg as Parameters<typeof this.handleControlRequest>[0]);
      } else {
        this.enqueue(msg);
      }
    });

    // Immediately check for startup failures — spawn() calls back with an
    // error event synchronously if the binary does not exist.
    this.proc.on("error", (err) => {
      process.stderr.write(`[NdjsonTransport] subprocess error: ${err.message}\n`);
    });

    this.proc.on("close", () => {
      this.closed = true;
    });
  }

  async send(msg: StdinMessage): Promise<void> {
    if (!this.proc?.stdin?.writable) {
      throw new Error("Transport not started");
    }
    const raw = JSON.stringify(msg)
      .replace(/\u2028/g, "\\u2028")
      .replace(/\u2029/g, "\\u2029");
    this.proc.stdin.write(raw + "\n");
  }

  async *recv(): AsyncGenerator<StdoutMessage> {
    while (!this.closed || this.queue.length > 0) {
      if (this.queue.length > 0) {
        yield this.queue.shift()!;
      } else {
        const msg = await new Promise<StdoutMessage>((resolve) => {
          this.waiters.push(resolve);
        });
        yield msg;
      }
    }
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;

    if (this.proc) {
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

    for (const waiter of this.waiters) {
      waiter({ type: "system", content: "transport closed" });
    }
    this.waiters = [];
  }

  private enqueue(msg: StdoutMessage): void {
    if (this.waiters.length > 0) {
      this.waiters.shift()!(msg);
    } else {
      this.queue.push(msg);
    }
  }

  private buildControlResponse(
    req: ControlRequestCanUseTool | ControlRequestSessionStateChanged | ControlRequestMcpSetServers,
    result: PermissionDecision,
  ): StdinMessage {
    // boolean → explicit allow/deny shape
    if (typeof result === "boolean") {
      return result
        ? ({ type: "control_response", request_id: req.request_id, decision: "allow" } as const)
        : ({
            type: "control_response",
            request_id: req.request_id,
            decision: "deny",
            reason: "denied by SDK callback",
          } as const);
    }
    // Already a typed shape
    return result as StdinMessage;
  }

  private async handleControlRequest(
    req: ControlRequestCanUseTool | ControlRequestSessionStateChanged | ControlRequestMcpSetServers,
  ): Promise<void> {
    if (!this.onPermission) return;

    let response: StdinMessage;
    try {
      const result = await Promise.resolve(this.onPermission(req));
      response = this.buildControlResponse(req, result);
    } catch {
      // Deny on exception — always safer than implicitly allowing.
      response = {
        type: "control_response",
        request_id: req.request_id,
        decision: "deny",
        reason: "permission callback raised an exception",
      } as const;
    }

    await this.send(response);
  }
}
