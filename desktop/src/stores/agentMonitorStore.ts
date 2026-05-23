// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { useMemo } from 'react'
import { useSessionStore } from './sessionStore'
import { useSessionRunStateStore } from './sessionRunStateStore'
import { useChatStore } from './chatStore'
import { useSettingsStore } from './settingsStore'
import { useWorkspaceQueueStore } from './workspaceQueueStore'
import { useTabStore } from './tabStore'
import {
  buildAgentSnapshot,
  compareAgentSnapshots,
  summarizeAgents,
  type AgentSnapshot,
  type AgentStatusSummary,
  type ResourceProfileInfo,
} from '../utils/agentStatus'
import type { CodingModeId } from '../types/codingMode'

const EXPANDED_KEY = 'sen-agent-monitor-expanded'
const FILTER_KEY = 'sen-agent-monitor-filter'
const GROUP_KEY = 'sen-agent-monitor-group'

export type AgentMonitorFilterMode = 'active' | 'errors'
export type AgentMonitorGroupBy = 'workspace' | 'status' | 'flat'

const VALID_FILTERS: AgentMonitorFilterMode[] = ['active', 'errors']
const VALID_GROUPS: AgentMonitorGroupBy[] = ['workspace', 'status', 'flat']

function migrateLegacyFilter(key: string): void {
  try {
    const raw = localStorage.getItem(key)
    if (raw === 'all') {
      localStorage.setItem(key, 'active')
    }
  } catch {
  }
}

migrateLegacyFilter(FILTER_KEY)

function readBoolean(key: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(key)
    if (raw === 'true') return true
    if (raw === 'false') return false
  } catch {
  }
  return fallback
}

function readEnum<T extends string>(key: string, allowed: T[], fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    if (raw && (allowed as string[]).includes(raw)) return raw as T
  } catch {
  }
  return fallback
}

function writeString(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
  }
}

type AgentMonitorStore = {
  expanded: boolean
  filterMode: AgentMonitorFilterMode
  groupBy: AgentMonitorGroupBy
  toggleExpanded: () => void
  setExpanded: (open: boolean) => void
  setFilterMode: (mode: AgentMonitorFilterMode) => void
  setGroupBy: (group: AgentMonitorGroupBy) => void
}

export const useAgentMonitorStore = create<AgentMonitorStore>((set) => ({
  expanded: readBoolean(EXPANDED_KEY, true),
  filterMode: readEnum<AgentMonitorFilterMode>(FILTER_KEY, VALID_FILTERS, 'active'),
  groupBy: readEnum<AgentMonitorGroupBy>(GROUP_KEY, VALID_GROUPS, 'workspace'),

  toggleExpanded: () =>
    set((state) => {
      const next = !state.expanded
      writeString(EXPANDED_KEY, next ? 'true' : 'false')
      return { expanded: next }
    }),

  setExpanded: (open) => {
    writeString(EXPANDED_KEY, open ? 'true' : 'false')
    set({ expanded: open })
  },

  setFilterMode: (mode) => {
    writeString(FILTER_KEY, mode)
    set({ filterMode: mode })
  },

  setGroupBy: (group) => {
    writeString(GROUP_KEY, group)
    set({ groupBy: group })
  },
}))

export function useAgentSnapshots(): AgentSnapshot[] {
  const sessions = useSessionStore((s) => s.sessions)
  const running = useSessionRunStateStore((s) => s.running)
  const chatSessions = useChatStore((s) => s.sessions)
  const sessionCodingMode = useChatStore((s) => s.sessionCodingMode)
  const queueState = useWorkspaceQueueStore((s) => s.queues)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const codingModeCatalog = useSettingsStore((s) => s.codingModes)
  const globalCodingMode = useSettingsStore((s) => s.codingMode)

  return useMemo(() => {
    const queueCounts: Record<string, number> = {}
    for (const list of Object.values(queueState)) {
      for (const item of list) {
        queueCounts[item.sessionId] = (queueCounts[item.sessionId] ?? 0) + 1
      }
    }

    const profileMap = new Map<CodingModeId, ResourceProfileInfo>()
    for (const info of codingModeCatalog) {
      if (info.resourceProfile) {
        profileMap.set(info.id, {
          browser: info.resourceProfile.browser,
          shell: info.resourceProfile.shell,
          mayWrite: info.resourceProfile.mayWrite,
        })
      }
    }

    const snapshots: AgentSnapshot[] = []
    for (const session of sessions) {
      const codingMode =
        sessionCodingMode[session.id] ?? globalCodingMode ?? null
      const resourceProfile = codingMode
        ? profileMap.get(codingMode) ?? null
        : null
      const snap = buildAgentSnapshot(
        {
          session,
          isRunning: running.has(session.id),
          chatSession: chatSessions[session.id] ?? null,
          queueLen: queueCounts[session.id] ?? 0,
          codingMode,
          resourceProfile,
        },
        { isAttached: session.id === activeTabId },
      )
      snapshots.push(snap)
    }
    snapshots.sort(compareAgentSnapshots)
    return snapshots
  }, [
    sessions,
    running,
    chatSessions,
    sessionCodingMode,
    queueState,
    activeTabId,
    codingModeCatalog,
    globalCodingMode,
  ])
}

export function useAgentSummary(snapshots: AgentSnapshot[]): AgentStatusSummary {
  return useMemo(() => summarizeAgents(snapshots), [snapshots])
}

export type AgentGroupBucket = {
  key: string
  label: string
  snapshots: AgentSnapshot[]
}

const WORKSPACE_UNKNOWN_KEY = '__unknown__'

function workspaceBasename(path: string): string {
  const trimmed = path.trim().replace(/[\\/]+$/, '')
  if (!trimmed) return ''
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

export function groupAgentSnapshots(
  snapshots: AgentSnapshot[],
  groupBy: AgentMonitorGroupBy,
  unknownLabel: string,
): AgentGroupBucket[] {
  if (groupBy === 'flat') {
    return [{ key: '__flat__', label: '', snapshots }]
  }

  if (groupBy === 'status') {
    const order: Array<{ key: AgentSnapshot['status']; label: string }> = [
      { key: 'error', label: 'error' },
      { key: 'waiting', label: 'waiting' },
      { key: 'waiting_resource', label: 'waiting_resource' },
      { key: 'tool', label: 'tool' },
      { key: 'thinking', label: 'thinking' },
      { key: 'running', label: 'running' },
      { key: 'queued', label: 'queued' },
      { key: 'disconnected', label: 'disconnected' },
      { key: 'missingWorkDir', label: 'missingWorkDir' },
      { key: 'idle', label: 'idle' },
    ]
    const map = new Map<string, AgentSnapshot[]>()
    for (const snap of snapshots) {
      const list = map.get(snap.status) ?? []
      list.push(snap)
      map.set(snap.status, list)
    }
    const out: AgentGroupBucket[] = []
    for (const item of order) {
      const list = map.get(item.key)
      if (list && list.length > 0) {
        out.push({ key: item.key, label: item.label, snapshots: list })
      }
    }
    return out
  }

  const buckets = new Map<string, AgentSnapshot[]>()
  for (const snap of snapshots) {
    const wd = snap.workDir?.trim() ?? ''
    const key = wd || WORKSPACE_UNKNOWN_KEY
    const list = buckets.get(key) ?? []
    list.push(snap)
    buckets.set(key, list)
  }

  const out: AgentGroupBucket[] = []
  for (const [key, list] of buckets) {
    const label =
      key === WORKSPACE_UNKNOWN_KEY ? unknownLabel : workspaceBasename(key) || key
    out.push({ key, label, snapshots: list })
  }
  out.sort((a, b) => {
    if (a.key === WORKSPACE_UNKNOWN_KEY) return 1
    if (b.key === WORKSPACE_UNKNOWN_KEY) return -1
    return a.label.localeCompare(b.label)
  })
  return out
}

export function filterAgentSnapshots(
  snapshots: AgentSnapshot[],
  filterMode: AgentMonitorFilterMode,
): AgentSnapshot[] {
  if (filterMode === 'errors') {
    return snapshots.filter((s) => s.status === 'error' || s.status === 'missingWorkDir')
  }
  return snapshots.filter(
    (s) =>
      s.status === 'running' ||
      s.status === 'thinking' ||
      s.status === 'tool' ||
      s.status === 'waiting' ||
      s.status === 'waiting_resource' ||
      s.status === 'queued',
  )
}
