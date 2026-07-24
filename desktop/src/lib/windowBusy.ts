// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const IDLE_MS = 180
const BUSY_CLASS = 'is-window-busy'

let busy = false
let idleTimer: ReturnType<typeof setTimeout> | undefined
const idleListeners = new Set<() => void>()

export function isWindowBusy(): boolean {
  return busy
}

export function onWindowIdle(listener: () => void): () => void {
  idleListeners.add(listener)
  return () => {
    idleListeners.delete(listener)
  }
}

export function markWindowBusy(): void {
  if (!busy) {
    busy = true
    if (typeof document !== 'undefined') {
      document.documentElement.classList.add(BUSY_CLASS)
    }
  }
  if (idleTimer !== undefined) {
    clearTimeout(idleTimer)
  }
  idleTimer = setTimeout(() => {
    idleTimer = undefined
    busy = false
    if (typeof document !== 'undefined') {
      document.documentElement.classList.remove(BUSY_CLASS)
    }
    for (const listener of idleListeners) {
      try {
        listener()
      } catch {
      }
    }
  }, IDLE_MS)
}
