// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useSyncExternalStore } from 'react'

const TICK_INTERVAL_MS = 1_000

let sharedNow = Date.now()
const listeners = new Set<() => void>()
let tickerTimer: ReturnType<typeof setInterval> | null = null

function ensureTicker() {
  if (tickerTimer !== null || listeners.size === 0) return
  sharedNow = Date.now()
  tickerTimer = setInterval(() => {
    sharedNow = Date.now()
    for (const listener of listeners) listener()
  }, TICK_INTERVAL_MS)
}

function stopTickerIfIdle() {
  if (tickerTimer !== null && listeners.size === 0) {
    clearInterval(tickerTimer)
    tickerTimer = null
  }
}

function subscribeShared(listener: () => void): () => void {
  listeners.add(listener)
  ensureTicker()
  return () => {
    listeners.delete(listener)
    stopTickerIfIdle()
  }
}

function subscribeNever(): () => void {
  return () => {}
}

function getSharedNow(): number {
  return sharedNow
}

export function useFreshnessNow(active: boolean): number {
  const ticking = useSyncExternalStore(
    active ? subscribeShared : subscribeNever,
    getSharedNow,
    getSharedNow,
  )
  return active ? ticking : Date.now()
}
