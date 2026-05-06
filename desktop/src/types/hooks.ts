export type WebhookAuditConfig = {
  enabled: boolean
  url: string
  toolPatterns: string[]
  includeArgs: boolean
  maxArgsBytes: number
}

export type BuiltinHooksConfig = {
  commandLogger: boolean
  webhookAudit: WebhookAuditConfig
}

export type HooksConfig = {
  enabled: boolean
  builtin: BuiltinHooksConfig
  scriptHookPaths: string[]
}

export type HooksPatch = {
  enabled?: boolean
  builtin?: {
    commandLogger?: boolean
    webhookAudit?: Partial<WebhookAuditConfig>
  }
}
