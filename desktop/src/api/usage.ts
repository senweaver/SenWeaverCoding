// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type {
  UsageCodingModeStats,
  UsageProviderStats,
  UsageSessionModelStats,
  UsageSessionStats,
  UsageSummary,
} from '../types/usage'

type RawProviderStats = {
  provider?: string
  costUsd?: number
  inputTokens?: number
  outputTokens?: number
  totalTokens?: number
  requestCount?: number
  modelCount?: number
  models?: string[]
  firstUsed?: string | null
  lastUsed?: string | null
}

type RawCodingModeStats = {
  mode?: string
  costUsd?: number
  inputTokens?: number
  outputTokens?: number
  totalTokens?: number
  requestCount?: number
  sessionCount?: number
  modelCount?: number
  firstUsed?: string | null
  lastUsed?: string | null
}

type RawSessionModelStats = {
  model?: string
  costUsd?: number
  inputTokens?: number
  outputTokens?: number
  totalTokens?: number
  requestCount?: number
  firstUsed?: string | null
  lastUsed?: string | null
}

type RawSessionStats = {
  sessionId?: string
  costUsd?: number
  inputTokens?: number
  outputTokens?: number
  totalTokens?: number
  requestCount?: number
  firstUsed?: string | null
  lastUsed?: string | null
  byModel?: Record<string, RawSessionModelStats>
}

type RawUsageResponse = {
  cost: {
    sessionCostUsd?: number
    dailyCostUsd?: number
    monthlyCostUsd?: number
    totalTokens?: number
    requestCount?: number
    byModel?: Record<string, {
      model: string
      cost_usd?: number
      costUsd?: number
      total_tokens?: number
      totalTokens?: number
      request_count?: number
      requestCount?: number
    }>
    byModelLifetime?: UsageSummary['byModelLifetime']
    bySession?: Record<string, RawSessionStats>
    byProvider?: Record<string, RawProviderStats>
    byCodingMode?: Record<string, RawCodingModeStats>
    tokenRatePerMin?: number
    last24hTokens?: number
    last24hCostUsd?: number
    last24hRequests?: number
    last7dTokens?: number
    last7dCostUsd?: number
    last7dRequests?: number
  }
}

function normaliseProvider(
  key: string,
  raw: RawProviderStats | undefined,
): UsageProviderStats {
  return {
    provider: raw?.provider ?? key,
    costUsd: raw?.costUsd ?? 0,
    inputTokens: raw?.inputTokens ?? 0,
    outputTokens: raw?.outputTokens ?? 0,
    totalTokens: raw?.totalTokens ?? 0,
    requestCount: raw?.requestCount ?? 0,
    modelCount: raw?.modelCount ?? raw?.models?.length ?? 0,
    models: Array.isArray(raw?.models) ? raw!.models! : [],
    firstUsed: raw?.firstUsed ?? null,
    lastUsed: raw?.lastUsed ?? null,
  }
}

function normaliseCodingMode(
  key: string,
  raw: RawCodingModeStats | undefined,
): UsageCodingModeStats {
  return {
    mode: raw?.mode ?? key,
    costUsd: raw?.costUsd ?? 0,
    inputTokens: raw?.inputTokens ?? 0,
    outputTokens: raw?.outputTokens ?? 0,
    totalTokens: raw?.totalTokens ?? 0,
    requestCount: raw?.requestCount ?? 0,
    sessionCount: raw?.sessionCount ?? 0,
    modelCount: raw?.modelCount ?? 0,
    firstUsed: raw?.firstUsed ?? null,
    lastUsed: raw?.lastUsed ?? null,
  }
}

function normaliseSessionModel(
  key: string,
  raw: RawSessionModelStats | undefined,
): UsageSessionModelStats {
  return {
    model: raw?.model ?? key,
    costUsd: raw?.costUsd ?? 0,
    inputTokens: raw?.inputTokens ?? 0,
    outputTokens: raw?.outputTokens ?? 0,
    totalTokens: raw?.totalTokens ?? 0,
    requestCount: raw?.requestCount ?? 0,
    firstUsed: raw?.firstUsed ?? null,
    lastUsed: raw?.lastUsed ?? null,
  }
}

function normaliseSession(
  sessionId: string,
  raw: RawSessionStats | undefined,
): UsageSessionStats {
  const byModel: Record<string, UsageSessionModelStats> = {}
  for (const [modelKey, modelRaw] of Object.entries(raw?.byModel ?? {})) {
    byModel[modelKey] = normaliseSessionModel(modelKey, modelRaw)
  }
  return {
    sessionId: raw?.sessionId ?? sessionId,
    costUsd: raw?.costUsd ?? 0,
    inputTokens: raw?.inputTokens ?? 0,
    outputTokens: raw?.outputTokens ?? 0,
    totalTokens: raw?.totalTokens ?? 0,
    requestCount: raw?.requestCount ?? 0,
    firstUsed: raw?.firstUsed ?? null,
    lastUsed: raw?.lastUsed ?? null,
    byModel,
  }
}

function normaliseSummary(response: RawUsageResponse): UsageSummary {
  const cost = response?.cost ?? ({} as RawUsageResponse['cost'])
  const byModel: UsageSummary['byModel'] = {}
  for (const [k, v] of Object.entries(cost.byModel ?? {})) {
    byModel[k] = {
      model: v.model ?? k,
      cost_usd: v.cost_usd ?? v.costUsd ?? 0,
      total_tokens: v.total_tokens ?? v.totalTokens ?? 0,
      request_count: v.request_count ?? v.requestCount ?? 0,
    }
  }
  const bySession: UsageSummary['bySession'] = {}
  for (const [sessionId, raw] of Object.entries(cost.bySession ?? {})) {
    bySession[sessionId] = normaliseSession(sessionId, raw)
  }
  const byProvider: UsageSummary['byProvider'] = {}
  for (const [providerKey, raw] of Object.entries(cost.byProvider ?? {})) {
    byProvider[providerKey] = normaliseProvider(providerKey, raw)
  }
  const byCodingMode: UsageSummary['byCodingMode'] = {}
  for (const [modeKey, raw] of Object.entries(cost.byCodingMode ?? {})) {
    byCodingMode[modeKey] = normaliseCodingMode(modeKey, raw)
  }
  return {
    sessionCostUsd: cost.sessionCostUsd ?? 0,
    dailyCostUsd: cost.dailyCostUsd ?? 0,
    monthlyCostUsd: cost.monthlyCostUsd ?? 0,
    totalTokens: cost.totalTokens ?? 0,
    requestCount: cost.requestCount ?? 0,
    byModel,
    byModelLifetime: cost.byModelLifetime ?? {},
    bySession,
    byProvider,
    byCodingMode,
    tokenRatePerMin: cost.tokenRatePerMin ?? 0,
    last24hTokens: cost.last24hTokens ?? 0,
    last24hCostUsd: cost.last24hCostUsd ?? 0,
    last24hRequests: cost.last24hRequests ?? 0,
    last7dTokens: cost.last7dTokens ?? 0,
    last7dCostUsd: cost.last7dCostUsd ?? 0,
    last7dRequests: cost.last7dRequests ?? 0,
  }
}

export const usageApi = {
  get: async (period: 'all' | 'session' = 'all'): Promise<UsageSummary> => {
    const response = await api.get<RawUsageResponse>(
      `/api/usage?period=${encodeURIComponent(period)}`,
    )
    return normaliseSummary(response)
  },
}
