// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type ProxyScope = 'environment' | 'internal' | 'services'

export type NetworkProxySettings = {
  enabled: boolean
  httpProxy: string | null
  httpsProxy: string | null
  allProxy: string | null
  noProxy: string[]
  scope: ProxyScope
  systemDetect: boolean
}

export type NetworkSettings = {
  proxy: NetworkProxySettings
}

export type CronSettings = {
  enabled: boolean
  catch_up_on_startup: boolean
  max_run_history: number
}

export type SandboxSettings = {
  enabled: boolean
  backend: string
  confineFilesystem: boolean
  availableBackends: string[]
}

export type ResourceLimitSettings = {
  maxMemoryMb: number | null
  maxCpuTimeSeconds: number | null
  maxSubprocesses: number | null
}

export type SecuritySettings = {
  sandbox: SandboxSettings
  resources: ResourceLimitSettings
}

export type SecuritySettingsUpdate = {
  sandbox: {
    enabled: boolean
    backend: string
    confineFilesystem: boolean
  }
  resources: ResourceLimitSettings
}

export type ServiceTokensStatus = {
  rpcTokenSet: boolean
  mcpSseTokenSet: boolean
}

export type ServiceTokensUpdate = {
  rpcToken?: string | null
  mcpSseToken?: string | null
}

export const systemSettingsApi = {
  getNetworkSettings: () => api.get<NetworkSettings>('/api/network-settings'),
  updateNetworkSettings: (payload: NetworkSettings) =>
    api.put<NetworkSettings>('/api/network-settings', payload),

  getCronSettings: () => api.get<CronSettings>('/api/cron/settings'),
  updateCronSettings: (patch: Partial<CronSettings>) =>
    api.patch<CronSettings>('/api/cron/settings', patch),

  getSecuritySettings: () => api.get<SecuritySettings>('/api/security-settings'),
  updateSecuritySettings: (payload: SecuritySettingsUpdate) =>
    api.put<SecuritySettings>('/api/security-settings', payload),

  getServiceTokens: () => api.get<ServiceTokensStatus>('/api/service-tokens'),
  updateServiceTokens: (payload: ServiceTokensUpdate) =>
    api.put<ServiceTokensStatus>('/api/service-tokens', payload),
}
