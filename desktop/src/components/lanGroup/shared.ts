// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { TranslationKey } from '../../i18n'
import type { LanGroupRole, LanPhase } from '../../types/lanGroup'

export function isTauriRuntime(): boolean {
  return (
    typeof window !== 'undefined' &&
    ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
  )
}

export function initials(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return '?'
  return trimmed.slice(0, 1).toUpperCase()
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)))
  return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`
}

export function roleRank(role: LanGroupRole): number {
  switch (role) {
    case 'owner':
      return 3
    case 'manager':
      return 2
    case 'member':
      return 1
    default:
      return 0
  }
}

export function canManage(role: LanGroupRole): boolean {
  return roleRank(role) >= roleRank('manager')
}

export function canContribute(role: LanGroupRole): boolean {
  return roleRank(role) >= roleRank('member')
}

export async function pickPath(directory: boolean): Promise<string | null> {
  if (isTauriRuntime()) {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory, multiple: false })
      return Array.isArray(selected) ? (selected[0] ?? null) : selected
    } catch {
      return null
    }
  }
  return window.prompt('Path') || null
}

export function formatDate(ms: number): string {
  if (!ms) return ''
  try {
    return new Date(ms).toLocaleDateString()
  } catch {
    return ''
  }
}

export const ROLE_LABEL: Record<LanGroupRole, TranslationKey> = {
  owner: 'lanGroup.roleOwner',
  manager: 'lanGroup.roleManager',
  member: 'lanGroup.roleMember',
  viewer: 'lanGroup.roleViewer',
}

export const TASK_STATUS_LABEL: Record<string, TranslationKey> = {
  todo: 'lanGroup.statusTodo',
  in_progress: 'lanGroup.statusInProgress',
  done: 'lanGroup.statusDone',
  blocked: 'lanGroup.statusBlocked',
}

export const PHASE_STATUS_LABEL: Record<string, TranslationKey> = {
  not_started: 'lanGroup.phaseNotStarted',
  in_progress: 'lanGroup.phaseInProgress',
  done: 'lanGroup.phaseDone',
  blocked: 'lanGroup.phaseBlocked',
}

export const PRIORITY_LABEL: Record<string, TranslationKey> = {
  low: 'lanGroup.priorityLow',
  medium: 'lanGroup.priorityMedium',
  high: 'lanGroup.priorityHigh',
  urgent: 'lanGroup.priorityUrgent',
}

const DEFAULT_PHASE_LABEL: Record<string, TranslationKey> = {
  requirements: 'lanGroup.phaseRequirements',
  design: 'lanGroup.phaseDesign',
  development: 'lanGroup.phaseDevelopment',
  testing: 'lanGroup.phaseTesting',
  deployment: 'lanGroup.phaseDeployment',
  maintenance: 'lanGroup.phaseMaintenance',
}

export function phaseLabel(phase: LanPhase, t: (key: TranslationKey) => string): string {
  const key = DEFAULT_PHASE_LABEL[phase.id]
  if (key) return t(key)
  return phase.name
}

export const TASK_STATUS_ORDER = ['todo', 'in_progress', 'done', 'blocked']
export const PRIORITY_ORDER = ['low', 'medium', 'high', 'urgent']
export const PHASE_STATUS_ORDER = ['not_started', 'in_progress', 'done', 'blocked']

export function statusColor(status: string): string {
  switch (status) {
    case 'done':
      return 'var(--color-success, #16a34a)'
    case 'in_progress':
      return 'var(--color-brand)'
    case 'blocked':
      return 'var(--color-error)'
    default:
      return 'var(--color-text-tertiary)'
  }
}

export function priorityColor(priority: string): string {
  switch (priority) {
    case 'urgent':
      return 'var(--color-error)'
    case 'high':
      return '#f59e0b'
    case 'low':
      return 'var(--color-text-tertiary)'
    default:
      return 'var(--color-brand)'
  }
}
