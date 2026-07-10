// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import type { AttachmentRef, DesignGenerationOptions } from '../types/chat'
import type { SessionListItem } from '../types/session'
import { useSessionStore } from './sessionStore'
import { useSessionRunStateStore } from './sessionRunStateStore'

const isWindows =
  typeof navigator !== 'undefined' && /Win/i.test(navigator.platform || '')

export function workspaceKeyFor(
  session: SessionListItem | null | undefined,
  sessionId?: string,
): string {
  if (!session) {
    const fallback = sessionId ?? '__unknown__'
    return `__solo::${fallback}`
  }
  const raw = (session.workDir ?? '').trim()
  if (!raw) {
    return `__solo::${session.id}`
  }
  const unified = raw.replace(/\\/g, '/').replace(/\/+$/, '')
  const finalKey = unified.length > 0 ? unified : raw.replace(/\\/g, '/')
  return isWindows ? finalKey.toLowerCase() : finalKey
}

export type QueuedItem = {
  id: string
  sessionId: string
  workspaceKey: string
  content: string
  attachments?: AttachmentRef[]
  options?: {
    displayContent?: string
    designGeneration?: DesignGenerationOptions
  }
  queuedAt: number
}

type WorkspaceQueueStore = {
  queues: Record<string, QueuedItem[]>
  expandedSessions: Set<string>
  keepDrainingSessions: Set<string>

  enqueue: (
    sessionId: string,
    content: string,
    attachments?: AttachmentRef[],
    options?: {
      displayContent?: string
      designGeneration?: DesignGenerationOptions
    },
  ) => string
  cancel: (itemId: string) => void
  cancelAllForSession: (sessionId: string) => void
  toggleExpanded: (sessionId: string) => void
  setExpanded: (sessionId: string, expanded: boolean) => void
  setKeepDraining: (sessionId: string, on: boolean) => void

  getQueueForSession: (sessionId: string) => QueuedItem[]
  getOtherSessionsQueueCount: (sessionId: string) => number
  getRunningSessionInWorkspace: (workspaceKey: string) => string | null
  popHead: (workspaceKey: string) => QueuedItem | null
  unshift: (item: QueuedItem) => void
}

let nextQueueId = 1
function generateQueueId(): string {
  nextQueueId += 1
  return `wq_${Date.now().toString(36)}_${nextQueueId.toString(36)}`
}

function workspaceKeyForSessionId(sessionId: string): string {
  const session =
    useSessionStore.getState().sessions.find((s) => s.id === sessionId) ?? null
  return workspaceKeyFor(session, sessionId)
}

export const useWorkspaceQueueStore = create<WorkspaceQueueStore>((set, get) => ({
  queues: {},
  expandedSessions: new Set<string>(),
  keepDrainingSessions: new Set<string>(),

  enqueue: (sessionId, content, attachments, options) => {
    const workspaceKey = workspaceKeyForSessionId(sessionId)
    const item: QueuedItem = {
      id: generateQueueId(),
      sessionId,
      workspaceKey,
      content,
      attachments,
      options,
      queuedAt: Date.now(),
    }
    set((s) => {
      const list = s.queues[workspaceKey] ?? []
      const expanded = new Set(s.expandedSessions)
      expanded.add(sessionId)
      return {
        queues: { ...s.queues, [workspaceKey]: [...list, item] },
        expandedSessions: expanded,
      }
    })
    return item.id
  },

  cancel: (itemId) => {
    set((s) => {
      let touched = false
      const next: Record<string, QueuedItem[]> = {}
      for (const [key, list] of Object.entries(s.queues)) {
        const filtered = list.filter((i) => i.id !== itemId)
        if (filtered.length !== list.length) {
          touched = true
          if (filtered.length > 0) next[key] = filtered
        } else {
          next[key] = list
        }
      }
      if (!touched) return s
      return { queues: next }
    })
  },

  cancelAllForSession: (sessionId) => {
    set((s) => {
      let touched = false
      const next: Record<string, QueuedItem[]> = {}
      for (const [key, list] of Object.entries(s.queues)) {
        const filtered = list.filter((i) => i.sessionId !== sessionId)
        if (filtered.length !== list.length) touched = true
        if (filtered.length > 0) next[key] = filtered
      }
      if (!touched) return s
      return { queues: next }
    })
  },

  toggleExpanded: (sessionId) => {
    set((s) => {
      const next = new Set(s.expandedSessions)
      if (next.has(sessionId)) next.delete(sessionId)
      else next.add(sessionId)
      return { expandedSessions: next }
    })
  },

  setExpanded: (sessionId, expanded) => {
    set((s) => {
      const has = s.expandedSessions.has(sessionId)
      if (expanded === has) return s
      const next = new Set(s.expandedSessions)
      if (expanded) next.add(sessionId)
      else next.delete(sessionId)
      return { expandedSessions: next }
    })
  },

  setKeepDraining: (sessionId, on) => {
    set((s) => {
      const has = s.keepDrainingSessions.has(sessionId)
      if (on === has) return s
      const next = new Set(s.keepDrainingSessions)
      if (on) next.add(sessionId)
      else next.delete(sessionId)
      return { keepDrainingSessions: next }
    })
  },

  getQueueForSession: (sessionId) => {
    const all = get().queues
    const out: QueuedItem[] = []
    for (const list of Object.values(all)) {
      for (const item of list) {
        if (item.sessionId === sessionId) out.push(item)
      }
    }
    return out
  },

  getOtherSessionsQueueCount: (sessionId) => {
    const wsKey = workspaceKeyForSessionId(sessionId)
    const list = get().queues[wsKey] ?? []
    let count = 0
    for (const item of list) {
      if (item.sessionId !== sessionId) count += 1
    }
    return count
  },

  getRunningSessionInWorkspace: (workspaceKey) => {
    const running = useSessionRunStateStore.getState().running
    if (running.size === 0) return null
    const sessions = useSessionStore.getState().sessions
    for (const s of sessions) {
      if (!running.has(s.id)) continue
      if (workspaceKeyFor(s) === workspaceKey) return s.id
    }
    return null
  },

  popHead: (workspaceKey) => {
    let removed: QueuedItem | null = null
    set((s) => {
      const list = s.queues[workspaceKey]
      if (!list || list.length === 0) return s
      const head = list[0]
      const rest = list.slice(1)
      removed = head ?? null
      const next = { ...s.queues }
      if (rest.length > 0) next[workspaceKey] = rest
      else delete next[workspaceKey]
      return { queues: next }
    })
    return removed
  },

  unshift: (item) => {
    set((s) => {
      const list = s.queues[item.workspaceKey] ?? []
      return { queues: { ...s.queues, [item.workspaceKey]: [item, ...list] } }
    })
  },
}))

export function useQueueLengthForSession(sessionId: string | null | undefined): number {
  return useWorkspaceQueueStore((s) => {
    if (!sessionId) return 0
    let count = 0
    for (const list of Object.values(s.queues)) {
      for (const item of list) {
        if (item.sessionId === sessionId) count += 1
      }
    }
    return count
  })
}

const lastDrainedBySession = new Map<string, { item: QueuedItem; drainedAt: number }>()
const DRAIN_REQUEUE_WINDOW_MS = 30_000

export function takeLastDrainedItem(sessionId: string): QueuedItem | null {
  const entry = lastDrainedBySession.get(sessionId)
  if (!entry) return null
  lastDrainedBySession.delete(sessionId)
  if (Date.now() - entry.drainedAt > DRAIN_REQUEUE_WINDOW_MS) return null
  return entry.item
}

export function requeueRejectedItem(item: QueuedItem): void {
  useWorkspaceQueueStore.getState().unshift(item)
}

export async function tryDrainWorkspace(workspaceKey: string): Promise<void> {
  const store = useWorkspaceQueueStore.getState()
  const running = useSessionRunStateStore.getState().running
  const list = store.queues[workspaceKey] ?? []
  const headIdx = list.findIndex((item) => !running.has(item.sessionId))
  if (headIdx < 0) return
  const head = list[headIdx]
  if (!head) return
  useWorkspaceQueueStore.setState((s) => {
    const arr = s.queues[workspaceKey] ?? []
    const next = arr.filter((item) => item.id !== head.id)
    const queues = { ...s.queues }
    if (next.length > 0) queues[workspaceKey] = next
    else delete queues[workspaceKey]
    return { queues }
  })
  lastDrainedBySession.set(head.sessionId, { item: head, drainedAt: Date.now() })
  const { useChatStore } = await import('./chatStore')
  useChatStore.getState().sendMessage(head.sessionId, head.content, head.attachments, {
    ...(head.options ?? {}),
    __internalDrain: true,
  })
}
