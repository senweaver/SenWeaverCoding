// SPDX-License-Identifier: MIT

import { useSessionRunStateStore } from '../stores/sessionRunStateStore'

export function waitForSessionsIdle(ids: string[], timeoutMs: number): Promise<void> {
  return new Promise((resolve) => {
    const allIdle = () =>
      ids.every((id) => !useSessionRunStateStore.getState().running.has(id))
    if (allIdle()) {
      resolve()
      return
    }
    let settled = false
    let timer: ReturnType<typeof setTimeout> | null = null
    const finish = () => {
      if (settled) return
      settled = true
      unsubscribe()
      if (timer) clearTimeout(timer)
      resolve()
    }
    const unsubscribe = useSessionRunStateStore.subscribe(() => {
      if (!settled && allIdle()) finish()
    })
    timer = setTimeout(finish, timeoutMs)
  })
}
