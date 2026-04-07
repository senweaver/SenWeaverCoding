export interface UserMessage {
    type: "user_message";
    content: string;
    images?: string[];
}
export interface ControlResponseAllow {
    type: "control_response";
    request_id: string;
    decision: "allow";
    updated_input?: unknown;
}
export interface ControlResponseDeny {
    type: "control_response";
    request_id: string;
    decision: "deny";
    reason?: string;
}
/** Union of the two valid ControlResponse shapes. */
export type ControlResponse = ControlResponseAllow | ControlResponseDeny;
export interface SdkMessage {
    type: "sdk_message";
    action: string;
    data?: unknown;
}
export type StdinMessage = UserMessage | ControlResponse | SdkMessage;
export interface AssistantMessage {
    type: "assistant_message";
    content: string;
    stop_reason?: string;
}
export interface ToolUseMessage {
    type: "tool_use";
    tool_use_id: string;
    tool_name: string;
    input: unknown;
}
export interface ToolResultMessage {
    type: "tool_result";
    tool_use_id: string;
    success: boolean;
    output: string;
    error?: string;
}
export interface ControlRequestCanUseTool {
    type: "control_request";
    request_id: string;
    action: "can_use_tool";
    tool_name: string;
    input: unknown;
    tool_use_id?: string;
}
export interface ControlRequestSessionStateChanged {
    type: "control_request";
    request_id: string;
    action: "session_state_changed";
    session_id: string;
    status?: string;
}
export interface ControlRequestMcpSetServers {
    type: "control_request";
    request_id: string;
    action: "mcp_set_servers";
    servers: unknown[];
}
/** Union of all valid ControlRequest shapes. */
export type ControlRequestMessage = ControlRequestCanUseTool | ControlRequestSessionStateChanged | ControlRequestMcpSetServers;
export interface SessionStateMessage {
    type: "session_state";
    session_id: string;
    status: string;
    metadata?: unknown;
}
export interface SystemMessage {
    type: "system";
    content: string;
}
export interface ResultMessage {
    type: "result";
    session_id?: string;
    cost?: number;
    duration_ms?: number;
    num_turns?: number;
}
export type StdoutMessage = AssistantMessage | ToolUseMessage | ToolResultMessage | ControlRequestMessage | SessionStateMessage | SystemMessage | ResultMessage;
export interface RpcRequest {
    jsonrpc: "2.0";
    method: string;
    params?: Record<string, unknown>;
    id?: string | number;
}
export interface RpcError {
    code: number;
    message: string;
    data?: unknown;
}
export interface RpcResponse {
    jsonrpc: "2.0";
    result?: unknown;
    error?: RpcError;
    id: string | number;
}
export interface HealthInfo {
    version: string;
    uptime_secs: number;
    memory_mb: number;
    active_sessions: number;
    status: string;
}
export interface SystemInfo {
    version: string;
    session_timeout_secs: number;
    max_sessions: number;
    active_sessions: number;
    enabled_transports: string[];
    workspace_dir: string;
}
export interface SessionInfo {
    id: string;
    created_at: string;
    last_active: string;
    workspace_dir: string;
}
