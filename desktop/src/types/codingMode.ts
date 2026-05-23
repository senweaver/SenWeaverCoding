

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
  | 'curator'

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
  'curator',
  'ask',
  'debug',
  'harness',
]

export const isVisibleCodingMode = (id: string): id is CodingModeId =>
  (VISIBLE_CODING_MODES as readonly string[]).includes(id)

export type CodingModeAccentTokens = {
  container: string
  onContainer: string
  accent: string
  accentHover: string
  onAccent: string
}

export const CODING_MODE_ACCENT: Partial<Record<CodingModeId, CodingModeAccentTokens>> = {
  agent: {
    container: 'var(--color-agent-accent-container)',
    onContainer: 'var(--color-on-agent-accent-container)',
    accent: 'var(--color-agent-accent)',
    accentHover: 'var(--color-agent-accent-hover)',
    onAccent: 'var(--color-on-agent-accent)',
  },
  spec: {
    container: 'var(--color-spec-accent-container)',
    onContainer: 'var(--color-on-spec-accent-container)',
    accent: 'var(--color-spec-accent)',
    accentHover: 'var(--color-spec-accent-hover)',
    onAccent: 'var(--color-on-spec-accent)',
  },
  plan: {
    container: 'var(--color-plan-accent-container)',
    onContainer: 'var(--color-on-plan-accent-container)',
    accent: 'var(--color-plan-accent)',
    accentHover: 'var(--color-plan-accent-hover)',
    onAccent: 'var(--color-on-plan-accent)',
  },
  curator: {
    container: 'var(--color-curator-accent-container)',
    onContainer: 'var(--color-on-curator-accent-container)',
    accent: 'var(--color-curator-accent)',
    accentHover: 'var(--color-curator-accent-hover)',
    onAccent: 'var(--color-on-curator-accent)',
  },
  ask: {
    container: 'var(--color-ask-accent-container)',
    onContainer: 'var(--color-on-ask-accent-container)',
    accent: 'var(--color-ask-accent)',
    accentHover: 'var(--color-ask-accent-hover)',
    onAccent: 'var(--color-on-ask-accent)',
  },
  debug: {
    container: 'var(--color-debug-accent-container)',
    onContainer: 'var(--color-on-debug-accent-container)',
    accent: 'var(--color-debug-accent)',
    accentHover: 'var(--color-debug-accent-hover)',
    onAccent: 'var(--color-on-debug-accent)',
  },
  harness: {
    container: 'var(--color-harness-accent-container)',
    onContainer: 'var(--color-on-harness-accent-container)',
    accent: 'var(--color-harness-accent)',
    accentHover: 'var(--color-harness-accent-hover)',
    onAccent: 'var(--color-on-harness-accent)',
  },
}
