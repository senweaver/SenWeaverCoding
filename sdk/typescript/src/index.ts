// SPDX-License-Identifier: MIT
// Public API surface for @senweaver/sen.

export {
  SenAgent,
  Session,
  RpcError,
  TransportError,
  type SenAgentOptions,
} from "./client.js";

export { NdjsonTransport, type NdjsonOptions } from "./ndjson.js";

export type {
  RpcRequest,
  RpcResponse,
  HealthInfo,
  SystemInfo,
  SessionInfo,
} from "./types.js";
