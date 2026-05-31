// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type {
  AgentCoreConfig,
  AgentCorePatch,
  AgentRuntimeConfig,
  AgentRuntimePatch,
  WebFetchPatch,
  WebFetchSettings,
  WebSearchPatch,
  WebSearchSettings,
} from '../types/agentSettings'

export const agentSettingsApi = {
  getAgentConfig: () => api.get<AgentCoreConfig>('/api/agent-config'),
  updateAgentConfig: (patch: AgentCorePatch) =>
    api.put<AgentCoreConfig>('/api/agent-config', patch),

  getAgentRuntime: () => api.get<AgentRuntimeConfig>('/api/agent-runtime'),
  updateAgentRuntime: (patch: AgentRuntimePatch) =>
    api.put<AgentRuntimeConfig>('/api/agent-runtime', patch),

  getWebSearch: () => api.get<WebSearchSettings>('/api/web-search'),
  updateWebSearch: (patch: WebSearchPatch) =>
    api.put<WebSearchSettings>('/api/web-search', patch),

  getWebFetch: () => api.get<WebFetchSettings>('/api/web-fetch'),
  updateWebFetch: (patch: WebFetchPatch) =>
    api.put<WebFetchSettings>('/api/web-fetch', patch),
}
