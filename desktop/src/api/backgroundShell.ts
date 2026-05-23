// SPDX-License-Identifier: MIT

import { getBaseUrl } from './client'
import {
  useTerminalPanelStore,
  type AgentMirrorEvent,
} from '../stores/terminalPanelStore'

let activeSource: EventSource | null = null
let starting = false
let retryTimer: ReturnType<typeof setTimeout> | null = null
let retryAttempt = 0

export function startBackgroundShellMirror(): void {
  if (typeof window === 'undefined') return
  if (typeof window.EventSource !== 'function') return
  if (activeSource || starting) return
  starting = true
  try {
    connect()
  } finally {
    starting = false
  }
}

export function stopBackgroundShellMirror(): void {
  if (retryTimer != null) {
    clearTimeout(retryTimer)
    retryTimer = null
  }
  if (activeSource) {
    try {
      activeSource.close()
    } catch {
    }
    activeSource = null
  }
}

function connect() {
  const url = `${getBaseUrl()}/api/background-shell/stream`
  let source: EventSource
  try {
    source = new window.EventSource(url, { withCredentials: false })
  } catch {
    scheduleReconnect()
    return
  }
  activeSource = source

  source.onopen = () => {
    retryAttempt = 0
  }

  source.onmessage = (msg: MessageEvent) => {
    if (typeof msg.data !== 'string' || msg.data.length === 0) return
    let parsed: unknown
    try {
      parsed = JSON.parse(msg.data)
    } catch {
      return
    }
    const event = parseSignal(parsed)
    if (!event) return
    useTerminalPanelStore.getState().appendAgentMirrorEvent(event)
  }

  source.onerror = () => {
    try {
      source.close()
    } catch {
    }
    if (activeSource === source) {
      activeSource = null
    }
    scheduleReconnect()
  }
}

function scheduleReconnect() {
  if (retryTimer != null) return
  retryAttempt = Math.min(retryAttempt + 1, 6)
  const delay = Math.min(15_000, 500 * 2 ** (retryAttempt - 1))
  retryTimer = setTimeout(() => {
    retryTimer = null
    connect()
  }, delay)
}

function parseSignal(raw: unknown): AgentMirrorEvent | null {
  if (!raw || typeof raw !== 'object') return null
  const obj = raw as Record<string, unknown>
  const type = obj.type
  if (typeof type !== 'string') return null
  const id = typeof obj.id === 'string' ? obj.id : ''
  if (!id) return null
  switch (type) {
    case 'spawned': {
      const command = typeof obj.command === 'string' ? obj.command : ''
      return { type: 'spawned', id, command }
    }
    case 'chunk': {
      const stream = obj.stream === 'stderr' ? 'stderr' : 'stdout'
      const line = typeof obj.line === 'string' ? obj.line : ''
      return { type: 'chunk', id, stream, line }
    }
    case 'heartbeat': {
      const elapsed = typeof obj.elapsedSecs === 'number' ? obj.elapsedSecs : 0
      return { type: 'heartbeat', id, elapsedSecs: elapsed }
    }
    case 'exited': {
      const elapsed = typeof obj.elapsedSecs === 'number' ? obj.elapsedSecs : 0
      const exitCode =
        typeof obj.exitCode === 'number'
          ? obj.exitCode
          : obj.exitCode === null
            ? null
            : null
      return { type: 'exited', id, elapsedSecs: elapsed, exitCode }
    }
    default:
      return null
  }
}
