// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type {
  RuntimeGatewayInfo,
  RuntimeSnapshot,
  RuntimeTaskGroup,
  RuntimeTasksInfo,
} from '../types/runtime'

type RawRuntimeTaskGroup = {
  name?: string
  count?: number
  oldestAgeMs?: number
}

type RawRuntimeTasks = {
  liveCount?: number
  groups?: RawRuntimeTaskGroup[]
}

type RawRuntimeGateway = {
  host?: string
  port?: number
  url?: string
  pathPrefix?: string
}

type RawRuntimeSnapshot = {
  version?: string
  buildProfile?: string
  pid?: number
  cpuCount?: number
  platform?: string
  arch?: string
  startedAt?: string
  uptimeSecs?: number
  workspaceDir?: string
  defaultProvider?: string | null
  defaultModel?: string | null
  gateway?: RawRuntimeGateway
  tasks?: RawRuntimeTasks
}

function normaliseGroup(raw: RawRuntimeTaskGroup): RuntimeTaskGroup {
  return {
    name: raw.name ?? '',
    count: Math.max(0, Math.floor(raw.count ?? 0)),
    oldestAgeMs: Math.max(0, Math.floor(raw.oldestAgeMs ?? 0)),
  }
}

function normaliseTasks(raw: RawRuntimeTasks | undefined): RuntimeTasksInfo {
  return {
    liveCount: Math.max(0, Math.floor(raw?.liveCount ?? 0)),
    groups: Array.isArray(raw?.groups) ? raw!.groups!.map(normaliseGroup) : [],
  }
}

function normaliseGateway(raw: RawRuntimeGateway | undefined): RuntimeGatewayInfo {
  return {
    host: raw?.host ?? '',
    port: Math.max(0, Math.floor(raw?.port ?? 0)),
    url: raw?.url ?? '',
    pathPrefix: raw?.pathPrefix ?? '',
  }
}

function normaliseSnapshot(raw: RawRuntimeSnapshot): RuntimeSnapshot {
  return {
    version: raw.version ?? '',
    buildProfile: raw.buildProfile ?? '',
    pid: Math.max(0, Math.floor(raw.pid ?? 0)),
    cpuCount: Math.max(0, Math.floor(raw.cpuCount ?? 0)),
    platform: raw.platform ?? '',
    arch: raw.arch ?? '',
    startedAt: raw.startedAt ?? '',
    uptimeSecs: Math.max(0, Math.floor(raw.uptimeSecs ?? 0)),
    workspaceDir: raw.workspaceDir ?? '',
    defaultProvider: raw.defaultProvider ?? null,
    defaultModel: raw.defaultModel ?? null,
    gateway: normaliseGateway(raw.gateway),
    tasks: normaliseTasks(raw.tasks),
  }
}

export const runtimeApi = {
  snapshot: async (): Promise<RuntimeSnapshot> => {
    const raw = await api.get<RawRuntimeSnapshot>('/api/runtime/snapshot')
    return normaliseSnapshot(raw)
  },
}
