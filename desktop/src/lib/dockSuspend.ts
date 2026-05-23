// SPDX-License-Identifier: MIT

import { dockHide, dockSetRect, type BrowserDockRect } from './browserDock'
import { useBrowserPanelStore } from '../stores/browserPanelStore'

let suspendCount = 0
let savedRect: BrowserDockRect | null = null
let suspendInFlight: Promise<void> | null = null

function snapshotActiveAnchorRect(): BrowserDockRect | null {
  const state = useBrowserPanelStore.getState()
  const sessionId = state.activeSessionId
  if (!sessionId) return null
  const panel = state.panels[sessionId]
  if (!panel || !panel.visible) return null
  const rect = panel.anchorRect
  if (!rect) return null
  return { x: rect.x, y: rect.y, w: rect.w, h: rect.h }
}

export function pushSuspend(): () => void {
  suspendCount += 1
  if (suspendCount === 1) {
    const rect = snapshotActiveAnchorRect()
    if (rect) {
      savedRect = rect
      suspendInFlight = (async () => {
        try {
          await dockHide()
        } catch (err) {
          console.warn('[dockSuspend] dockHide failed', err)
        }
      })()
    } else {
      savedRect = null
    }
  }

  let released = false
  return () => {
    if (released) return
    released = true
    suspendCount = Math.max(0, suspendCount - 1)
    if (suspendCount === 0) {
      const rect = savedRect
      savedRect = null
      const inFlight = suspendInFlight
      suspendInFlight = null
      if (rect) {
        void (async () => {
          try {
            if (inFlight) {
              await inFlight
            }
            await dockSetRect(rect)
          } catch (err) {
            console.warn('[dockSuspend] dockSetRect restore failed', err)
          }
        })()
      }
    }
  }
}

export function isDockSuspended(): boolean {
  return suspendCount > 0
}
