import type { HealthInfo, SystemInfo, SessionInfo } from "./types.js";
export declare class RpcError extends Error {
    readonly code: number;
    readonly data?: unknown | undefined;
    constructor(message: string, code: number, data?: unknown | undefined);
}
export declare class TransportError extends Error {
    constructor(message: string);
}
interface Transport {
    call(method: string, params?: Record<string, unknown>): Promise<unknown>;
    close(): Promise<void>;
}
export interface SenAgentOptions {
    httpUrl?: string;
    transport?: "stdio";
    senBinary?: string;
    auth?: string;
    timeout?: number;
}
export declare class SenAgent {
    private readonly transport;
    constructor(options: SenAgentOptions);
    health(): Promise<HealthInfo>;
    systemInfo(): Promise<SystemInfo>;
    createSession(opts?: {
        workspaceDir?: string;
        systemPrompt?: string;
    }): Promise<Session>;
    listSessions(): Promise<SessionInfo[]>;
    close(): Promise<void>;
}
export declare class Session {
    private readonly transport;
    readonly id: string;
    constructor(transport: Transport, id: string);
    prompt(message: string, timeout?: number): Promise<string>;
    stop(): Promise<void>;
    kill(): Promise<void>;
    executeTool(name: string, args?: Record<string, unknown>): Promise<unknown>;
    memoryStore(content: string, opts?: {
        category?: string;
        importance?: number;
        tags?: string[];
    }): Promise<string>;
    memoryRecall(query: string, opts?: {
        limit?: number;
        category?: string;
    }): Promise<Array<Record<string, unknown>>>;
    blackboardPut(key: string, value: unknown, opts?: {
        namespace?: string;
    }): Promise<void>;
    blackboardGet(key: string, opts?: {
        namespace?: string;
    }): Promise<unknown>;
    blackboardList(opts?: {
        namespace?: string;
    }): Promise<string[]>;
}
export {};
