"use strict";
// SPDX-License-Identifier: MIT
// NDJSON transport for headless / SDK mode.
// Speaks the streaming NDJSON protocol (--output-format stream-json), NOT JSON-RPC.
Object.defineProperty(exports, "__esModule", { value: true });
exports.NdjsonTransport = void 0;
const node_child_process_1 = require("node:child_process");
const node_readline_1 = require("node:readline");
class NdjsonTransport {
    proc = null;
    queue = [];
    waiters = [];
    closed = false;
    binary;
    args;
    cwd;
    env;
    onPermission;
    constructor(options) {
        this.binary = options?.binary ?? "sen";
        this.args = options?.args ?? [];
        this.cwd = options?.cwd;
        this.env = options?.env;
        this.onPermission = options?.onPermission;
    }
    async start() {
        const cmd = ["agent", "--output-format", "stream-json", ...this.args];
        this.proc = (0, node_child_process_1.spawn)(this.binary, cmd, {
            // Do NOT merge with process.env when env is empty — that causes the
            // subprocess to lose PATH and other essential variables on Windows.
            stdio: ["pipe", "pipe", "pipe"],
            cwd: this.cwd,
            env: this.env ? { ...process.env, ...this.env } : undefined,
        });
        // Drain stderr to prevent the subprocess from blocking on EPIPE.
        this.proc.stderr?.on("data", (chunk) => {
            process.stderr.write(chunk);
        });
        const rl = (0, node_readline_1.createInterface)({ input: this.proc.stdout });
        rl.on("line", (line) => {
            const trimmed = line.trim();
            if (!trimmed)
                return;
            let data;
            try {
                data = JSON.parse(trimmed);
            }
            catch {
                return;
            }
            const msg = data;
            if (msg.type === "control_request" && this.onPermission) {
                void this.handleControlRequest(msg);
            }
            else {
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
    async send(msg) {
        if (!this.proc?.stdin?.writable) {
            throw new Error("Transport not started");
        }
        const raw = JSON.stringify(msg)
            .replace(/\u2028/g, "\\u2028")
            .replace(/\u2029/g, "\\u2029");
        this.proc.stdin.write(raw + "\n");
    }
    async *recv() {
        while (!this.closed || this.queue.length > 0) {
            if (this.queue.length > 0) {
                yield this.queue.shift();
            }
            else {
                const msg = await new Promise((resolve) => {
                    this.waiters.push(resolve);
                });
                yield msg;
            }
        }
    }
    async close() {
        if (this.closed)
            return;
        this.closed = true;
        if (this.proc) {
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
        for (const waiter of this.waiters) {
            waiter({ type: "system", content: "transport closed" });
        }
        this.waiters = [];
    }
    enqueue(msg) {
        if (this.waiters.length > 0) {
            this.waiters.shift()(msg);
        }
        else {
            this.queue.push(msg);
        }
    }
    buildControlResponse(req, result) {
        // boolean → explicit allow/deny shape
        if (typeof result === "boolean") {
            return result
                ? { type: "control_response", request_id: req.request_id, decision: "allow" }
                : {
                    type: "control_response",
                    request_id: req.request_id,
                    decision: "deny",
                    reason: "denied by SDK callback",
                };
        }
        // Already a typed shape
        return result;
    }
    async handleControlRequest(req) {
        if (!this.onPermission)
            return;
        let response;
        try {
            const result = await Promise.resolve(this.onPermission(req));
            response = this.buildControlResponse(req, result);
        }
        catch {
            // Deny on exception — always safer than implicitly allowing.
            response = {
                type: "control_response",
                request_id: req.request_id,
                decision: "deny",
                reason: "permission callback raised an exception",
            };
        }
        await this.send(response);
    }
}
exports.NdjsonTransport = NdjsonTransport;
