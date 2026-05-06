import { api } from './client'
import type { GuardrailRule, GuardrailsConfig, RulePolicy } from '../types/rules'

type RawRule = {
  tool_pattern?: string
  toolPattern?: string
  policy: RulePolicy
  reason: string | null
  contexts: string[]
}

type RawGuardrails = {
  enabled: boolean
  default_policy?: RulePolicy
  defaultPolicy?: RulePolicy
  rules_count?: number
  rulesCount?: number
  rate_limits_count?: number
  rateLimitsCount?: number
  max_calls_per_session?: number
  maxCallsPerSession?: number
  bypass_tools?: string[]
  bypassTools?: string[]
  rules: RawRule[]
}

function normalise(raw: RawGuardrails): GuardrailsConfig {
  return {
    enabled: raw.enabled,
    defaultPolicy: raw.default_policy ?? raw.defaultPolicy ?? 'allow',
    rulesCount: raw.rules_count ?? raw.rulesCount ?? raw.rules?.length ?? 0,
    rateLimitsCount: raw.rate_limits_count ?? raw.rateLimitsCount ?? 0,
    maxCallsPerSession: raw.max_calls_per_session ?? raw.maxCallsPerSession ?? 0,
    bypassTools: raw.bypass_tools ?? raw.bypassTools ?? [],
    rules: (raw.rules ?? []).map<GuardrailRule>((r) => ({
      toolPattern: r.tool_pattern ?? r.toolPattern ?? '',
      policy: r.policy,
      reason: r.reason ?? null,
      contexts: r.contexts ?? [],
    })),
  }
}

export type GuardrailsPatch = {
  enabled?: boolean
  defaultPolicy?: RulePolicy
  bypassTools?: string[]
  maxCallsPerSession?: number
  rules?: GuardrailRule[]
}

export const guardrailsApi = {
  get: async (): Promise<GuardrailsConfig> => {
    const raw = await api.get<RawGuardrails>('/api/guardrails')
    return normalise(raw)
  },
  update: async (patch: GuardrailsPatch): Promise<{ status: string }> =>
    api.put<{ status: string }>('/api/guardrails', patch),
}
