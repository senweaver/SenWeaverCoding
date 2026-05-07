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

export type UsageProviderStats = {
  provider: string
  costUsd: number
  inputTokens: number
  outputTokens: number
  totalTokens: number
  requestCount: number
  modelCount: number
  models: string[]
  firstUsed: string | null
  lastUsed: string | null
}

export type UsageCodingModeStats = {
  mode: string
  costUsd: number
  inputTokens: number
  outputTokens: number
  totalTokens: number
  requestCount: number
  sessionCount: number
  modelCount: number
  firstUsed: string | null
  lastUsed: string | null
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
  byProvider: Record<string, UsageProviderStats>
  byCodingMode: Record<string, UsageCodingModeStats>
  tokenRatePerMin: number
  last24hTokens: number
  last24hCostUsd: number
  last24hRequests: number
  last7dTokens: number
  last7dCostUsd: number
  last7dRequests: number
}
