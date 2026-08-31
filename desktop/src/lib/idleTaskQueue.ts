// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { isScrollActive } from './scrollActivity'

const idleTaskQueue: Array<() => void> = []
let idlePumpScheduled = false

function pumpIdleTaskQueue() {
  idlePumpScheduled = false
  if (isScrollActive()) {
    if (idleTaskQueue.length > 0) scheduleIdlePump()
    return
  }
  const task = idleTaskQueue.shift()
  if (task) task()
  if (idleTaskQueue.length > 0) scheduleIdlePump()
}

function scheduleIdlePump() {
  if (idlePumpScheduled) return
  idlePumpScheduled = true
  type IdleFn = (cb: () => void, options?: { timeout?: number }) => number
  const ric = (window as unknown as { requestIdleCallback?: IdleFn }).requestIdleCallback
  if (typeof ric === 'function') {
    ric(pumpIdleTaskQueue, { timeout: 500 })
  } else {
    setTimeout(pumpIdleTaskQueue, 80)
  }
}

export function enqueueIdleTask(
  task: () => void,
  options?: { front?: boolean },
): () => void {
  if (typeof window === 'undefined') {
    task()
    return () => {}
  }
  if (options?.front) {
    idleTaskQueue.unshift(task)
  } else {
    idleTaskQueue.push(task)
  }
  scheduleIdlePump()
  return () => {
    const idx = idleTaskQueue.indexOf(task)
    if (idx >= 0) idleTaskQueue.splice(idx, 1)
  }
}
