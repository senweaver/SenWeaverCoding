// SPDX-License-Identifier: MIT
//
// Computes how far from the viewport's right edge a fixed-position
// floater (toast, update banner, …) must be inset so it does not slide
// underneath the embedded Tauri browser dock.  Returns 0 when no dock
// is active for the current session.  Reactive to both
// `browserPanelStore` updates AND viewport resizes — the dock's anchor
// rect is in physical pixels but `window.innerWidth` is in CSS pixels;
// since Tauri always sets `devicePixelRatio` to 1 on the main webview
// they coincide, so we can compare directly.

import { useEffect, useState } from 'react'

import { useBrowserPanelStore } from '../stores/browserPanelStore'

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
  const activeSessionId = useBrowserPanelStore((s) => s.activeSessionId)
  const dockVisible = useBrowserPanelStore((s) =>
    activeSessionId ? s.panels[activeSessionId]?.visible ?? false : false,
  )
  const dockX = useBrowserPanelStore((s) =>
    activeSessionId ? s.panels[activeSessionId]?.anchorRect?.x ?? null : null,
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
