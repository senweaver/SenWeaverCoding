

import type { PermissionMode } from './settings'

export type CodingModeId =
  | 'vibe'
  | 'spec'
  | 'plan'
  | 'ask'
  | 'tdd'
  | 'debug'
  | 'agent'
  | 'architect'
  | 'pair'
  | 'context'
  | 'mvai'
  | 'harness'

export type CodingModeInfo = {
  id: CodingModeId

  label: string

  description: string

  icon: string

  permissionMode: PermissionMode
}

export const DEFAULT_CODING_MODE: CodingModeId = 'vibe'
