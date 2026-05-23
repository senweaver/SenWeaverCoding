// SPDX-License-Identifier: MIT

import { useEffect, useState } from 'react'

import { useBrowserPanelStore } from '../stores/browserPanelStore'
import { useTabStore } from '../stores/tabStore'

function computeOffset(
  dockVisible: boolean,
  dockX: number | null,
  viewportWidth: number,
): number {
  if (!dockVisible || dockX === null) return 0
  const offset = viewportWidth - dockX
  return offset > 0 ? Math.round(offset) : 0
}

export function useDockEdgeOffset(): number {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const dockVisible = useBrowserPanelStore((s) =>
    activeTabId ? s.panels[activeTabId]?.visible ?? false : false,
  )
  const dockX = useBrowserPanelStore((s) =>
    activeTabId ? s.panels[activeTabId]?.anchorRect?.x ?? null : null,
  )

  const [viewportWidth, setViewportWidth] = useState(() =>
    typeof window === 'undefined' ? 0 : window.innerWidth,
  )

  useEffect(() => {
    if (typeof window === 'undefined') return
    const handleResize = () => setViewportWidth(window.innerWidth)
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  return computeOffset(dockVisible, dockX, viewportWidth)
}
