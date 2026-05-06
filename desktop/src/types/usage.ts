export type UsageModelStats = {
  model: string
  cost_usd: number
  total_tokens: number
  request_count: number
}

export type UsageLifetimeStats = {
  model: string
  costUsd: number
  inputTokens: number
  outputTokens: number
  totalTokens: number
  requestCount: number
  firstUsed: string | null
  lastUsed: string | null
}

export type UsageSessionModelStats = {
  model: string
  costUsd: number
  inputTokens: number
  outputTokens: number
  totalTokens: number
  requestCount: number
  firstUsed: string | null
  lastUsed: string | null
}

export type UsageSessionStats = {
  sessionId: string
  costUsd: number
  inputTokens: number
  outputTokens: number
  totalTokens: number
  requestCount: number
  firstUsed: string | null
  lastUsed: string | null
  byModel: Record<string, UsageSessionModelStats>
}

export type UsageSummary = {
  sessionCostUsd: number
  dailyCostUsd: number
  monthlyCostUsd: number
  totalTokens: number
  requestCount: number
  byModel: Record<string, UsageModelStats>
  byModelLifetime: Record<string, UsageLifetimeStats>
  bySession: Record<string, UsageSessionStats>
}
