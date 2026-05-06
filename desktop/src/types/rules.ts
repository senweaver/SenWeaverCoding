export type RulePolicy = 'allow' | 'deny' | 'require_approval' | 'audit_only'

export type GuardrailRule = {
  toolPattern: string
  policy: RulePolicy
  reason: string | null
  contexts: string[]
}

export type GuardrailsConfig = {
  enabled: boolean
  defaultPolicy: RulePolicy
  rulesCount: number
  rateLimitsCount: number
  maxCallsPerSession: number
  bypassTools: string[]
  rules: GuardrailRule[]
}
