"use strict";
// SPDX-License-Identifier: MIT
// SenWeaverCoding TypeScript SDK — main client (JSON-RPC transport).
Object.defineProperty(exports, "__esModule", { value: true });
exports.Session = exports.SenAgent = exports.TransportError = exports.RpcError = void 0;
const node_child_process_1 = require("node:child_process");
const node_readline_1 = require("node:readline");
const node_crypto_1 = require("node:crypto");
// ── Errors ──────────────────────────────────────────────────────────
class RpcError extends Error {
    code;
    data;
    constructor(message, code, data) {
        super(message);
        this.code = code;
        this.data = data;
        this.name = "RpcError";
    }
}
exports.RpcError = RpcError;
class TransportError extends Error {
    constructor(message) {
        super(message);
        this.name = "TransportError";
    }
}
exports.TransportError = TransportError;
// ── HttpTransport ───────────────────────────────────────────────────
class HttpTransport {
    url;
    headers;
    timeout;
    constructor(url, auth, timeout = 120_000) {
        this.url = url.replace(/\/+$/, "") + "/rpc";
        this.headers = { "Content-Type": "application/json" };
        if (auth)
            this.headers["Authorization"] = `Bearer ${auth}`;
        this.timeout = timeout;
    }
    async call(method, params) {
        const body = {
            jsonrpc: "2.0",
            method,
            id: (0, node_crypto_1.randomUUID)(),
        };
        if (params)
            body.params = params;
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), this.timeout);
        let res;
        try {
            res = await fetch(this.url, {
                method: "POST",
                headers: this.headers,
                body: JSON.stringify(body),
                signal: controller.signal,
            });
        }
        catch (err) {
            throw new TransportError(`HTTP request failed: ${err instanceof Error ? err.message : err}`);
        }
        finally {
            clearTimeout(timer);
        }
        if (!res.ok) {
            throw new TransportError(`HTTP ${res.status}: ${res.statusText}`);
        }
        const data = (await res.json());
        if (data.error) {
            throw new RpcError(data.error.message, data.error.code, data.error.data);
        }
        return data.result;
    }
    async close() {
        /* HTTP is stateless */
    }
}
// ── StdioTransport ──────────────────────────────────────────────────
class StdioTransport {
    binary;
    proc = null;
    pending = new Map();
    constructor(binary) {
        this.binary = binary;
    }
    start() {
        this.proc = (0, node_child_process_1.spawn)(this.binary, ["rpc", "--stdio"], {
            stdio: ["pipe", "pipe", "pipe"],
        });
        const rl = (0, node_readline_1.createInterface)({ input: this.proc.stdout });
        rl.on("line", (line) => {
            if (!line.trim())
                return;
            let data;
            try {
                data = JSON.parse(line);
            }
            catch {
                return;
            }
            const id = String(data.id);
            const pending = this.pending.get(id);
            if (!pending)
                return;
            this.pending.delete(id);
            if (data.error) {
                pending.reject(new RpcError(data.error.message, data.error.code, data.error.data));
            }
            else {
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
    async call(method, params) {
        if (!this.proc?.stdin?.writable) {
            throw new TransportError("Subprocess not started");
        }
        const id = (0, node_crypto_1.randomUUID)();
        const body = { jsonrpc: "2.0", method, id };
        if (params)
            body.params = params;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.proc.stdin.write(JSON.stringify(body) + "\n", (err) => {
                if (err) {
                    this.pending.delete(id);
                    reject(new TransportError(`Write failed: ${err.message}`));
                }
            });
        });
    }
    async close() {
        if (!this.proc)
            return;
        this.proc.kill("SIGTERM");
        await new Promise((resolve) => {
            const timer = setTimeout(() => {
                this.proc?.kill("SIGKILL");
                resolve();
            }, 5_000);
            this.proc.on("close", () => {
                clearTimeout(timer);
                resolve();
            });
        });
        this.proc = null;
    }
}
class SenAgent {
    transport;
    constructor(options) {
        if (options.httpUrl) {
            this.transport = new HttpTransport(options.httpUrl, options.auth, options.timeout);
        }
        else if (options.transport === "stdio") {
            const binary = options.senBinary ??
                process.env["SEN_BIN"] ??
                "sen";
            const stdio = new StdioTransport(binary);
            stdio.start();
            this.transport = stdio;
        }
        else {
            throw new Error('Must provide either httpUrl or transport: "stdio"');
        }
    }
    async health() {
        const raw = (await this.transport.call("system.health"));
        return {
            version: String(raw["version"] ?? "unknown"),
            uptime_secs: Number(raw["uptimeSecs"] ?? 0),
            memory_mb: Number(raw["memoryMb"] ?? 0),
            active_sessions: Number(raw["activeSessions"] ?? 0),
            status: String(raw["status"] ?? "unknown"),
        };
    }
    async systemInfo() {
        const raw = (await this.transport.call("system.info"));
        return {
            version: String(raw["version"] ?? "unknown"),
            session_timeout_secs: Number(raw["sessionTimeoutSecs"] ?? 300),
            max_sessions: Number(raw["maxSessions"] ?? 100),
            active_sessions: Number(raw["activeSessions"] ?? 0),
            enabled_transports: raw["enabledTransports"] ?? [],
            workspace_dir: String(raw["workspaceDir"] ?? ""),
        };
    }
    async createSession(opts) {
        const params = {};
        if (opts?.workspaceDir)
            params["workspaceDir"] = opts.workspaceDir;
        if (opts?.systemPrompt)
            params["systemPrompt"] = opts.systemPrompt;
        const raw = (await this.transport.call("session.new", params));
        return new Session(this.transport, String(raw["sessionId"]));
    }
    async listSessions() {
        const raw = (await this.transport.call("session.list"));
        const sessions = raw["sessions"] ?? [];
        return sessions.map((s) => ({
            id: String(s["sessionId"]),
            created_at: String(s["createdAt"]),
            last_active: String(s["lastActive"]),
            workspace_dir: String(s["workspaceDir"] ?? ""),
        }));
    }
    async close() {
        await this.transport.close();
    }
}
exports.SenAgent = SenAgent;
// ── Session ─────────────────────────────────────────────────────────
class Session {
    transport;
    id;
    constructor(transport, id) {
        this.transport = transport;
        this.id = id;
    }
    async prompt(message, timeout = 300_000) {
        const result = (await Promise.race([
            this.transport.call("session.prompt", {
                sessionId: this.id,
                message,
            }),
            new Promise((_, reject) => setTimeout(() => reject(new Error(`Agent did not respond within ${timeout}ms`)), timeout)),
        ]));
        return String(result["response"] ?? "");
    }
    async stop() {
        await this.transport.call("session.stop", { sessionId: this.id });
    }
    async kill() {
        await this.transport.call("session.kill", { sessionId: this.id });
    }
    async executeTool(name, args) {
        return this.transport.call("tool.exec", {
            sessionId: this.id,
            tool: name,
            args: args ?? {},
        });
    }
    // ── Memory ────────────────────────────────────────────────────────
    async memoryStore(content, opts) {
        const params = {
            content,
            category: opts?.category ?? "experience",
        };
        if (opts?.importance !== undefined)
            params["importance"] = opts.importance;
        if (opts?.tags)
            params["tags"] = opts.tags;
        const raw = (await this.transport.call("memory.store", params));
        return String(raw["id"] ?? "");
    }
    async memoryRecall(query, opts) {
        const params = { query, limit: opts?.limit ?? 5 };
        if (opts?.category)
            params["category"] = opts.category;
        const raw = (await this.transport.call("memory.recall", params));
        return raw["memories"] ?? [];
    }
    // ── Blackboard ──────────────────────────────────────────────────
    async blackboardPut(key, value, opts) {
        await this.transport.call("blackboard.put", {
            key,
            value,
            namespace: opts?.namespace ?? "default",
        });
    }
    async blackboardGet(key, opts) {
        const raw = (await this.transport.call("blackboard.get", {
            key,
            namespace: opts?.namespace ?? "default",
        }));
        return raw["value"];
    }
    async blackboardList(opts) {
        const raw = (await this.transport.call("blackboard.list", {
            namespace: opts?.namespace ?? "default",
        }));
        return raw["keys"] ?? [];
    }
}
exports.Session = Session;
