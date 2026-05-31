// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { getBaseUrl } from '../api/client'

const RECONNECT_DELAYS_MS = [1000, 2000, 5000, 10000, 30000]

type SessionRunStateInternal = {
  eventSource: EventSource | null
  reconnectTimer: number | null
  reconnectAttempt: number
  starting: boolean
}

type SessionRunStateStore = {
  running: Set<string>
  esConnected: boolean
  setSnapshot: (ids: string[]) => void
  mergeIds: (ids: string[]) => void
  applyDelta: (sessionId: string, running: boolean) => void
  start: () => void
  stop: () => void
}

const internal: SessionRunStateInternal = {
  eventSource: null,
  reconnectTimer: null,
  reconnectAttempt: 0,
  starting: false,
}

function clearReconnectTimer() {
  if (internal.reconnectTimer !== null) {
    window.clearTimeout(internal.reconnectTimer)
    internal.reconnectTimer = null
  }
}

function closeEventSource() {
  if (internal.eventSource) {
    try {
      internal.eventSource.close()
    } catch {

    }
    internal.eventSource = null
  }
}

function scheduleReconnect() {
  clearReconnectTimer()
  const attempt = Math.min(internal.reconnectAttempt, RECONNECT_DELAYS_MS.length - 1)
  const delay = RECONNECT_DELAYS_MS[attempt] ?? 30000
  internal.reconnectAttempt = Math.min(internal.reconnectAttempt + 1, RECONNECT_DELAYS_MS.length - 1)
  internal.reconnectTimer = window.setTimeout(() => {
    internal.reconnectTimer = null
    openStream()
  }, delay)
}

function openStream() {
  closeEventSource()
  const base = getBaseUrl().replace(/\/$/, '')
  const url = `${base}/api/sessions/events`
  let es: EventSource
  try {
    es = new EventSource(url)
  } catch {
    scheduleReconnect()
    return
  }
  internal.eventSource = es

  es.addEventListener('snapshot', (raw) => {
    try {
      const ev = raw as MessageEvent<string>
      const parsed = JSON.parse(ev.data) as { running?: unknown }
      if (Array.isArray(parsed.running)) {
        const ids = parsed.running.filter((x): x is string => typeof x === 'string')
        useSessionRunStateStore.getState().setSnapshot(ids)
      }
    } catch {

    }
  })

  es.addEventListener('run_state', (raw) => {
    try {
      const ev = raw as MessageEvent<string>
      const parsed = JSON.parse(ev.data) as { sessionId?: unknown; running?: unknown }
      if (typeof parsed.sessionId === 'string' && typeof parsed.running === 'boolean') {
        useSessionRunStateStore.getState().applyDelta(parsed.sessionId, parsed.running)
      }
    } catch {

    }
  })

  es.onopen = () => {
    internal.reconnectAttempt = 0
    useSessionRunStateStore.setState({ esConnected: true })
  }

  es.onerror = () => {
    useSessionRunStateStore.setState({ esConnected: false })
    closeEventSource()
    scheduleReconnect()
  }
}

export const useSessionRunStateStore = create<SessionRunStateStore>((set, get) => ({
  running: new Set<string>(),
  esConnected: false,
  setSnapshot: (ids) => {
    const previous = get().running
    set({ running: new Set(ids) })
    const nextSet = new Set(ids)
    for (const prev of previous) {
      if (!nextSet.has(prev)) {
        void scheduleQueueDrainForSession(prev)
      }
    }
  },
  mergeIds: (ids) => {
    if (!ids || ids.length === 0) return
    const current = get().running
    let changed = false
    const next = new Set(current)
    for (const id of ids) {
      if (!next.has(id)) {
        next.add(id)
        changed = true
      }
    }
    if (changed) {
      set({ running: next })
    }
  },
  applyDelta: (sessionId, running) => {
    const current = get().running
    if (running) {
      if (current.has(sessionId)) return
      const next = new Set(current)
      next.add(sessionId)
      set({ running: next })
    } else {
      if (!current.has(sessionId)) return
      const next = new Set(current)
      next.delete(sessionId)
      set({ running: next })
      void scheduleQueueDrainForSession(sessionId)
    }
  },
  start: () => {
    if (internal.starting) return
    if (internal.eventSource) return
    internal.starting = true
    try {
      internal.reconnectAttempt = 0
      openStream()
    } finally {
      internal.starting = false
    }
  },
  stop: () => {
    clearReconnectTimer()
    closeEventSource()
    internal.reconnectAttempt = 0
    set({ esConnected: false })
  },
}))

export function useIsSessionRunning(sessionId: string | null | undefined): boolean {
  return useSessionRunStateStore((s) => (sessionId ? s.running.has(sessionId) : false))
}

async function scheduleQueueDrainForSession(sessionId: string): Promise<void> {
  try {
    const [{ useSessionStore }, { useWorkspaceQueueStore, workspaceKeyFor, tryDrainWorkspace }] =
      await Promise.all([import('./sessionStore'), import('./workspaceQueueStore')])
    const session =
      useSessionStore.getState().sessions.find((s) => s.id === sessionId) ?? null
    const wsKey = workspaceKeyFor(session, sessionId)
    const queueLen = useWorkspaceQueueStore.getState().queues[wsKey]?.length ?? 0
    if (queueLen === 0) return
    queueMicrotask(() => {
      void tryDrainWorkspace(wsKey)
    })
  } catch {

  }
}
