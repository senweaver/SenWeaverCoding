// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export type ThinkingLevel = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'max'

export type ThinkingConfig = {
  defaultLevel: ThinkingLevel
}

export type HistoryPrunerConfig = {
  enabled: boolean
  maxTokens: number
  keepRecent: number
  collapseToolResults: boolean
}

export type ContextCompressionConfig = {
  enabled: boolean
  thresholdRatio: number
  protectFirstN: number
  protectLastN: number
  maxPasses: number
  summaryMaxChars: number
  sourceMaxChars: number
  timeoutSecs: number
  summaryModel: string | null
  identifierPolicy: string
}

export type EvalConfig = {
  enabled: boolean
  minQualityScore: number
  maxRetries: number
}

export type GlobalDirective = {
  content: string
  mode: string | null
}

export type AutoIndexConfig = {
  enabled: boolean
  includePatterns: string[]
  excludePatterns: string[]
  maxFiles: number
  refreshIntervalSecs: number
}

export type AgentCoreConfig = {
  compactContext: boolean
  maxToolIterations: number
  maxHistoryMessages: number
  maxContextTokens: number
  parallelTools: boolean
  toolDispatcher: string
  toolCallDedupExempt: string[]
  toolFilterGroups: unknown[]
  maxSystemPromptChars: number
  thinking: ThinkingConfig
  historyPruning: HistoryPrunerConfig
  contextAwareTools: boolean
  eval: EvalConfig
  autoClassify: unknown
  contextCompression: ContextCompressionConfig
  globalDirectives: GlobalDirective[]
  projectConfigDir: string | null
  autoIndex: AutoIndexConfig
  builtinToolDeferredLoading: boolean
}

export type SubagentLimitConfig = {
  maxConcurrent?: number
  globalCap?: number
}

export type SelfConsistencyConfig = {
  enabled: boolean
  samples: number
  temperature: number
  maxConcurrent: number
  finalOnly: boolean
}

export type AgentRuntimeConfig = {
  maxToolIterations: number
  loopDetectionThreshold: number
  parallelToolsEnabled: boolean
  perTurnTokenSoftCap: number
  perTurnTokenHardCap: number
  maxSubagents: number
  parallelToolMaxConcurrency: number
  subagentLimit: SubagentLimitConfig
  subagentCallTimeoutSecs: number
  fastApplyModel: string | null
  fastApplyTemperature: number
  fastApplyTimeoutSecs: number
  selfConsistency: SelfConsistencyConfig
}

export type FirecrawlConfig = {
  enabled: boolean
  apiKeyEnv: string
  apiUrl: string
  mode: string
}

export type WebSearchSettings = {
  enabled: boolean
  provider: string
  braveApiKey: string | null
  searxngInstanceUrl: string | null
  tavilyApiKey: string | null
  exaApiKey: string | null
  maxResults: number
  timeoutSecs: number
}

export type WebFetchSettings = {
  enabled: boolean
  allowedDomains: string[]
  blockedDomains: string[]
  allowedPrivateHosts: string[]
  maxResponseSize: number
  timeoutSecs: number
  firecrawl: FirecrawlConfig
}

export type AgentCorePatch = {
  compactContext?: boolean
  maxToolIterations?: number
  maxHistoryMessages?: number
  maxContextTokens?: number
  parallelTools?: boolean
  toolDispatcher?: string
  maxSystemPromptChars?: number
  contextAwareTools?: boolean
  thinking?: Partial<ThinkingConfig>
  historyPruning?: Partial<HistoryPrunerConfig>
  eval?: Partial<EvalConfig>
  contextCompression?: Partial<ContextCompressionConfig>
  globalDirectives?: GlobalDirective[]
  projectConfigDir?: string | null
  autoIndex?: Partial<AutoIndexConfig>
  builtinToolDeferredLoading?: boolean
}

export type AgentRuntimePatch = {
  maxToolIterations?: number
  loopDetectionThreshold?: number
  parallelToolsEnabled?: boolean
  perTurnTokenSoftCap?: number
  perTurnTokenHardCap?: number
  maxSubagents?: number
  parallelToolMaxConcurrency?: number
  subagentCallTimeoutSecs?: number
  fastApplyModel?: string | null
  fastApplyTemperature?: number
  fastApplyTimeoutSecs?: number
  selfConsistency?: Partial<SelfConsistencyConfig>
}

export type WebSearchPatch = Partial<Omit<WebSearchSettings, 'braveApiKey' | 'searxngInstanceUrl' | 'tavilyApiKey' | 'exaApiKey'>> & {
  braveApiKey?: string | null
  searxngInstanceUrl?: string | null
  tavilyApiKey?: string | null
  exaApiKey?: string | null
}

export type WebFetchPatch = {
  enabled?: boolean
  allowedDomains?: string[]
  blockedDomains?: string[]
  allowedPrivateHosts?: string[]
  maxResponseSize?: number
  timeoutSecs?: number
  firecrawl?: Partial<FirecrawlConfig>
}
