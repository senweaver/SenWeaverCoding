import type { StdinMessage, StdoutMessage, ControlRequestCanUseTool, ControlRequestSessionStateChanged, ControlRequestMcpSetServers, ControlResponseAllow, ControlResponseDeny } from "./types.js";
export type PermissionDecision = ControlResponseAllow | ControlResponseDeny | boolean;
export interface NdjsonOptions {
    binary?: string;
    args?: string[];
    cwd?: string;
    env?: Record<string, string>;
    onPermission?: (req: ControlRequestCanUseTool | ControlRequestSessionStateChanged | ControlRequestMcpSetServers) => PermissionDecision | Promise<PermissionDecision>;
}
export declare class NdjsonTransport {
    private proc;
    private queue;
    private waiters;
    private closed;
    private readonly binary;
    private readonly args;
    private readonly cwd?;
    private readonly env?;
    private readonly onPermission?;
    constructor(options?: NdjsonOptions);
    start(): Promise<void>;
    send(msg: StdinMessage): Promise<void>;
    recv(): AsyncGenerator<StdoutMessage>;
    close(): Promise<void>;
    private enqueue;
    private buildControlResponse;
    private handleControlRequest;
}
