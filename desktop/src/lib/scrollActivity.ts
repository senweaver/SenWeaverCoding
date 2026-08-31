// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const QUIET_MS = 160

let lastActivityAt = 0
let chatScroller: HTMLElement | null = null
let widthObserver: ResizeObserver | null = null
let lastWidth = 0

type WidthListener = (width: number) => void
const widthListeners = new Set<WidthListener>()

export function notifyScrollActivity(): void {
  lastActivityAt = performance.now()
}

export function isScrollActive(): boolean {
  return performance.now() - lastActivityAt < QUIET_MS
}

export function registerChatScroller(el: HTMLElement | null): void {
  chatScroller = el
  if (widthObserver) {
    widthObserver.disconnect()
    widthObserver = null
  }
  if (el && typeof ResizeObserver !== 'undefined') {
    const initial = Math.round(el.clientWidth)
    if (initial > 0 && initial !== lastWidth) {
      lastWidth = initial
      for (const listener of widthListeners) listener(initial)
    }
    widthObserver = new ResizeObserver(() => {
      const w = Math.round(el.clientWidth)
      if (w > 0 && w !== lastWidth) {
        lastWidth = w
        for (const listener of widthListeners) listener(w)
      }
    })
    widthObserver.observe(el)
  }
}

export function getChatScrollerWidth(): number {
  return lastWidth
}

export function onChatScrollerWidthChange(listener: WidthListener): () => void {
  widthListeners.add(listener)
  if (lastWidth > 0) listener(lastWidth)
  return () => {
    widthListeners.delete(listener)
  }
}

export function isNearChatTop(px: number): boolean {
  return chatScroller !== null && chatScroller.scrollTop < px
}

export function runWhenScrollQuiet(
  cb: () => void,
  checkMs = 120,
  maxWaitMs = 400,
): () => void {
  if (!isScrollActive() || isNearChatTop(600)) {
    cb()
    return () => {}
  }
  const started = performance.now()
  const timer = window.setInterval(() => {
    if (
      !isScrollActive() ||
      isNearChatTop(600) ||
      performance.now() - started >= maxWaitMs
    ) {
      window.clearInterval(timer)
      cb()
    }
  }, checkMs)
  return () => window.clearInterval(timer)
}

export function waitForScrollQuiet(maxWaitMs: number): Promise<number> {
  if (!isScrollActive() || isNearChatTop(600)) return Promise.resolve(0)
  const started = performance.now()
  return new Promise((resolve) => {
    const timer = window.setInterval(() => {
      const waited = performance.now() - started
      if (!isScrollActive() || isNearChatTop(600) || waited >= maxWaitMs) {
        window.clearInterval(timer)
        resolve(Math.round(waited))
      }
    }, 90)
  })
}
