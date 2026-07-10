// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useChatStore } from '../stores/chatStore'
import { useWorkspaceFilesStore } from '../stores/workspaceFilesStore'
import type { PerSessionState } from '../stores/chatStore'
import type { UIMessage } from '../types/chat'

type SessionWatchState = { seen: Set<string>; scannedLen: number }
const watchStateBySession: Map<string, SessionWatchState> = new Map()

const MUTATION_TOOLS = new Set([
  'file_write',
  'Write',
  'file_create',
  'file_edit',
  'fileedit',
  'editfile',
  'multi_edit',
  'patch_apply',
  'glob_edit',
  'notebook_edit',
])

let unsubscribe: (() => void) | null = null

export function startAiWriteWatcher(): () => void {
  if (unsubscribe) return unsubscribe

  const initial = useChatStore.getState().sessions
  for (const [sessionId, state] of Object.entries(initial)) {
    primeSeenIds(sessionId, state.messages)
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

function primeSeenIds(sessionId: string, messages: readonly UIMessage[]) {
  let ws = watchStateBySession.get(sessionId)
  if (!ws) {
    ws = { seen: new Set(), scannedLen: 0 }
    watchStateBySession.set(sessionId, ws)
  }
  for (const msg of messages) {
    if (msg.type === 'file_edit') {
      ws.seen.add(msg.id)
    }
    if (msg.type === 'tool_result') {
      ws.seen.add(`tool_result:${msg.toolUseId}`)
    }
  }
  ws.scannedLen = messages.length
}

function handleSessions(
  sessions: Record<string, PerSessionState>,
  prevSessions: Record<string, PerSessionState>,
) {
  const root = useWorkspaceFilesStore.getState().root
  if (!root) return

  const notify = useWorkspaceFilesStore.getState().notifyAiFileChanged

  for (const [sessionId, state] of Object.entries(sessions)) {
    const messages = state.messages
    const prevMessages = prevSessions[sessionId]?.messages
    if (messages === prevMessages) continue

    let ws = watchStateBySession.get(sessionId)
    if (!ws) {
      ws = { seen: new Set(), scannedLen: 0 }
      watchStateBySession.set(sessionId, ws)
    }
    // Only scan the newly-appended tail. The persistent `seen` set (kept per
    // session rather than copied per array) plus the `scannedLen` cursor turns
    // the previous O(n) full scan + O(n) set copy on every update into O(delta).
    let start = ws.scannedLen
    if (start > messages.length) {
      // Array was replaced/shrank (rewind, reload); rescan from the top. The
      // `seen` set still dedups already-notified entries.
      start = 0
    }

    let toolPaths: Map<string, string[]> | null = null

    for (let i = start; i < messages.length; i++) {
      const msg = messages[i]
      if (!msg) continue
      if (msg.type === 'file_edit') {
        if (ws.seen.has(msg.id)) continue
        ws.seen.add(msg.id)
        const rel = normalizeRelPath(msg.path, root)
        if (rel) {
          notify(rel)
        }
        continue
      }

      if (msg.type === 'tool_result') {
        const key = `tool_result:${msg.toolUseId}`
        if (ws.seen.has(key)) continue
        if (msg.isError) continue
        ws.seen.add(key)
        if (!toolPaths) toolPaths = buildToolPathIndex(messages)
        const paths = toolPaths.get(msg.toolUseId)
        if (!paths) continue
        for (const rawPath of paths) {
          const rel = normalizeRelPath(rawPath, root)
          if (rel) {
            notify(rel)
          }
        }
      }
    }
    ws.scannedLen = messages.length
  }

  // Drop watch state for sessions that no longer exist to avoid unbounded growth.
  if (watchStateBySession.size > Object.keys(sessions).length) {
    for (const key of watchStateBySession.keys()) {
      if (!(key in sessions)) {
        watchStateBySession.delete(key)
      }
    }
  }
}

function buildToolPathIndex(messages: readonly UIMessage[]): Map<string, string[]> {
  const map = new Map<string, string[]>()
  for (const msg of messages) {
    if (msg.type !== 'tool_use') continue
    if (!MUTATION_TOOLS.has(msg.toolName)) continue
    const paths = extractMutationPaths(msg.toolName, msg.input)
    if (paths.length > 0) {
      map.set(msg.toolUseId, paths)
    }
  }
  return map
}

function extractMutationPaths(toolName: string, input: unknown): string[] {
  if (!input || typeof input !== 'object') return []
  const obj = input as Record<string, unknown>

  if (toolName === 'multi_edit' || toolName === 'glob_edit') {
    const edits = obj.edits
    if (Array.isArray(edits)) {
      const paths: string[] = []
      for (const edit of edits) {
        if (!edit || typeof edit !== 'object') continue
        const path = (edit as Record<string, unknown>).path
        if (typeof path === 'string' && path.length > 0) {
          paths.push(path)
        }
      }
      if (paths.length > 0) return paths
    }
  }

  if (typeof obj.path === 'string' && obj.path.length > 0) return [obj.path]
  if (typeof obj.file_path === 'string' && obj.file_path.length > 0) return [obj.file_path]
  return []
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
