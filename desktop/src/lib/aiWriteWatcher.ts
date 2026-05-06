// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Bridges the chat WebSocket's `file_edit` events into the workspace
// file store so the right-sidebar Monaco editor can distinguish
// AI-driven edits from out-of-band external changes.
//
// Wiring:
//
//   chatStore.sessions[*].messages    →  scan for new {type:'file_edit'}
//                                        ↓
//                                        normalizeRelPath(path, root)
//                                        ↓
//                                        workspaceFilesStore.registerAiPendingWrite
//
// The watcher is a no-op while no workspace root is mounted, so
// running the agent without the right sidebar costs nothing.  It is
// started once from `AppShell.tsx` on app boot and lives for the
// lifetime of the renderer; the returned dispose closure is wired
// into React's effect-cleanup path so HMR doesn't double-subscribe.

import { useChatStore } from '../stores/chatStore'
import { useWorkspaceFilesStore } from '../stores/workspaceFilesStore'
import type { PerSessionState } from '../stores/chatStore'
import type { UIMessage } from '../types/chat'

const seenIdsByMessages: WeakMap<readonly UIMessage[], Set<string>> = new WeakMap()

let unsubscribe: (() => void) | null = null

export function startAiWriteWatcher(): () => void {
  if (unsubscribe) return unsubscribe

  const initial = useChatStore.getState().sessions
  for (const state of Object.values(initial)) {
    primeSeenIds(state.messages)
  }

  unsubscribe = useChatStore.subscribe((state, prev) => {
    handleSessions(state.sessions, prev.sessions)
  })

  return () => {
    if (unsubscribe) {
      unsubscribe()
      unsubscribe = null
    }
  }
}

function primeSeenIds(messages: readonly UIMessage[]) {
  let seen = seenIdsByMessages.get(messages)
  if (!seen) {
    seen = new Set()
    seenIdsByMessages.set(messages, seen)
  }
  for (const msg of messages) {
    if (msg.type === 'file_edit') {
      seen.add(msg.id)
    }
  }
}

function handleSessions(
  sessions: Record<string, PerSessionState>,
  prevSessions: Record<string, PerSessionState>,
) {
  const root = useWorkspaceFilesStore.getState().root
  if (!root) return

  const register = useWorkspaceFilesStore.getState().registerAiPendingWrite

  for (const [sessionId, state] of Object.entries(sessions)) {
    const messages = state.messages
    const prevMessages = prevSessions[sessionId]?.messages
    if (messages === prevMessages) continue

    let seen = seenIdsByMessages.get(messages)
    if (!seen) {

      const prevSeen = prevMessages ? seenIdsByMessages.get(prevMessages) : undefined
      seen = new Set(prevSeen ?? [])
      seenIdsByMessages.set(messages, seen)
    }

    for (const msg of messages) {
      if (msg.type !== 'file_edit') continue
      if (seen.has(msg.id)) continue
      seen.add(msg.id)
      const rel = normalizeRelPath(msg.path, root)
      if (rel) {
        register(rel)
      }
    }
  }
}

export function normalizeRelPath(rawPath: string, root: string): string | null {
  if (!rawPath) return null
  const normRoot = root.replace(/\\/g, '/').replace(/\/$/, '')
  let p = rawPath.replace(/\\/g, '/').replace(/\/$/, '')

  if (!p.includes('/') && !p.includes(':')) {

    return p
  }

  const isWindowsAbs = /^[a-zA-Z]:/.test(p)
  const lhs = isWindowsAbs ? p.toLowerCase() : p
  const rhs = isWindowsAbs ? normRoot.toLowerCase() : normRoot

  if (lhs === rhs) return null
  if (lhs.startsWith(rhs + '/')) {
    return p.slice(normRoot.length + 1)
  }

  if (!p.startsWith('/') && !isWindowsAbs) {
    return p
  }

  return null
}
