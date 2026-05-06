

export type PermissionMode =
  | 'default'
  | 'acceptEdits'
  | 'plan'
  | 'bypassPermissions'
  | 'dontAsk'
  | 'askEveryTime'

export type AutonomySettings = {
  autoApprove: string[]
  alwaysAsk: string[]
  protectBrowserTools: boolean
  protectMcpTools: boolean
  autoApproveModeTransitions: string[]
}

export type EffortLevel = 'low' | 'medium' | 'high' | 'max'
export type ThemeMode = 'light' | 'dark'

export type ModelInfo = {
  id: string
  name: string
  description: string
  context: string
}

export type UserSettings = {
  model?: string
  modelContext?: string
  effort?: EffortLevel
  permissionMode?: PermissionMode
  theme?: ThemeMode
  [key: string]: unknown
}
