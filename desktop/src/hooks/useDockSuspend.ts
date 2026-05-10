// SPDX-License-Identifier: MIT

import { useEffect } from 'react'

import { pushSuspend } from '../lib/dockSuspend'

export function useDockSuspend(active: boolean): void {
  useEffect(() => {
    if (!active) return
    const release = pushSuspend()
    return release
  }, [active])
}
