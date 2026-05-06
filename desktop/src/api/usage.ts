import { api } from './client'
import type {
  UsageSessionModelStats,
  UsageSessionStats,
  UsageSummary,
} from '../types/usage'

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
  return {
    sessionCostUsd: cost.sessionCostUsd ?? 0,
    dailyCostUsd: cost.dailyCostUsd ?? 0,
    monthlyCostUsd: cost.monthlyCostUsd ?? 0,
    totalTokens: cost.totalTokens ?? 0,
    requestCount: cost.requestCount ?? 0,
    byModel,
    byModelLifetime: cost.byModelLifetime ?? {},
    bySession,
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
