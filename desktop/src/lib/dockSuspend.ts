// SPDX-License-Identifier: MIT
//
// Process-global suspend/resume mechanism for the embedded Tauri browser
// dock.  The dock is an OS-level child WebView that always paints above
// the main app's HTML, so any HTML overlay (modal, full-screen confirm,
// toast positioned to the right edge) that overlaps the dock's rect
// would otherwise be physically obscured.  Modals invoke `pushSuspend`
// when they mount; on the 0->1 transition we snapshot the dock's
// current rect and call `dockHide()` to slide all tab webviews off
// screen while keeping their state alive.  When the last suspender
// releases (1->0) we call `dockSetRect(snapshot)` to restore the dock
// to exactly where it was — the underlying tab is preserved so the
// user does not see a navigation reset.
//
// `useDockSuspend(active)` is the React hook wrapper; pass `true`
// while the overlay is mounted/visible and the hook will manage
// reference-counted lifecycle automatically.

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
