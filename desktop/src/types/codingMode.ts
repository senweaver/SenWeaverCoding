

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

export type CodingModeResourceProfile = {
  browser: boolean
  shell: boolean
  mayWrite: boolean
}

export type CodingModeInfo = {
  id: CodingModeId

  label: string

  description: string

  icon: string

  permissionMode: PermissionMode

  allowedTools?: string[]

  resourceProfile?: CodingModeResourceProfile
}

export const DEFAULT_CODING_MODE: CodingModeId = 'agent'

export const VISIBLE_CODING_MODES: CodingModeId[] = [
  'agent',
  'spec',
  'plan',
  'ask',
  'debug',
  'harness',
]

export const isVisibleCodingMode = (id: string): id is CodingModeId =>
  (VISIBLE_CODING_MODES as readonly string[]).includes(id)
