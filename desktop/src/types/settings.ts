// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



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
  enableCommandPolicy: boolean
}

export type LoopControlsSettings = {
  selfEvalEnabled: boolean
  evaluateCodeEdits: boolean
  evaluatorModel: string
  maxEvaluatorRetries: number
  frozenRubricPath: string
  maxCostPerDayCents: number
  estopEnabled: boolean
  costTrackingEnabled: boolean
}

export type EffortLevel = 'low' | 'medium' | 'high' | 'max'
export type ThemeMode = 'light' | 'dark'
export type CloseBehavior = 'minimize' | 'exit' | 'ask'

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
  closeBehavior?: CloseBehavior
  lanNickname?: string
  lanEmail?: string | null
  lanDiscoveryEnabled?: boolean
  [key: string]: unknown
}
