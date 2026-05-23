// SPDX-License-Identifier: MIT

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useTabStore } from '../../stores/tabStore'
import { useChatStore } from '../../stores/chatStore'
import {
  type BrowserAgentActionEntry,
  type BrowserConsoleEntry,
  type BrowserInspectorSnapshot,
  type BrowserUserActionEntry,
  BROWSER_COLUMN_WIDTH_BOUNDS,
  useBrowserPanelStore,
} from '../../stores/browserPanelStore'
import { useTeamStore } from '../../stores/teamStore'
import { useUIStore } from '../../stores/uiStore'
import {
  clampRectToHost,
  dockFocusActive,
  dockHide,
  dockOpen,
  dockPark,
  dockPresentSession,
  dockResync,
  dockScreenshot,
  dockSetRect,
  listenDockEvents,
} from '../../lib/browserDock'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { bindDebugTab, unbindDebugTab } from '../../lib/debugTabBind'
import { useTranslation } from '../../i18n'
import { BrowserPanelSplitter } from './BrowserPanelSplitter'

const AGENT_LIVE_WINDOW_MS = 1500
const AGENT_BUBBLE_WINDOW_MS = 2200
const VIEWPORT_MIN_PX = 200
const HEADER_PX = 32
const TOOLBAR_PX = 38
const TABBAR_PX = 34

function clampZoom(z: number): number {
  if (!Number.isFinite(z) || z <= 0) return 1
  return Math.min(3, Math.max(0.25, z))
}

type AgentBubble = { id: number; kind: string; ts: number }

function summarizeAgentArgs(args: unknown): string {
  if (args == null) return ''
  if (typeof args === 'string') return args.length > 80 ? `${args.slice(0, 80)}…` : args
  try {
    const text = JSON.stringify(args)
    return text.length > 80 ? `${text.slice(0, 80)}…` : text
  } catch {
    return String(args)
  }
}

export function EmbeddedBrowserPanel() {
  const t = useTranslation()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const memberInfo = useTeamStore((s) => (activeTabId ? s.getMemberBySessionId(activeTabId) : null))
  const isMemberSession = !!memberInfo
  const sessionId = activeTabId

  const panel = useBrowserPanelStore((s) => (sessionId ? s.panels[sessionId] : undefined))
  const activeSessionId = useBrowserPanelStore((s) => s.activeSessionId)
  const ensure = useBrowserPanelStore((s) => s.ensure)
  const setAnchorRect = useBrowserPanelStore((s) => s.setAnchorRect)
  const ingestEvent = useBrowserPanelStore((s) => s.ingestEvent)

  const chatState = useChatStore((s) =>
    sessionId ? s.sessions[sessionId]?.chatState : undefined,
  )
  const activeToolName = useChatStore((s) =>
    sessionId ? s.sessions[sessionId]?.activeToolName : undefined,
  )

  const visible = panel?.visible ?? false
  const url = panel?.url ?? ''
  const liveUrl = panel?.liveUrl ?? ''
  const title = panel?.title ?? ''
  const consoleOpen = panel?.consoleOpen ?? false
  const inspectorOpen = panel?.inspectorOpen ?? false
  const pickMode = panel?.pickMode ?? false
  const zoom = panel?.zoom ?? 1
  const consoleLog = panel?.consoleLog ?? []
  const inspector = panel?.inspector ?? null
  const driverOpen = panel?.driverOpen ?? false
  const agentLog = panel?.agentLog ?? []
  const userLog = panel?.userLog ?? []
  const lastAgentActionAt = panel?.lastAgentActionAt ?? 0
  const drawerHeightRatio = panel?.drawerHeightRatio ?? 0.35
  const columnWidthAuto = panel?.columnWidthAuto ?? true
  const ownsDock = sessionId !== null && activeSessionId === sessionId
  const hasContent = Boolean(
    (panel?.tabs?.length ?? 0) > 0 ||
    panel?.activeTabId != null ||
    (liveUrl && liveUrl.trim()) ||
    (url && url.trim()),
  )

  const [liveTick, setLiveTick] = useState(0)
  useEffect(() => {
    if (!lastAgentActionAt) return
    const elapsed = Date.now() - lastAgentActionAt
    if (elapsed >= AGENT_LIVE_WINDOW_MS) return
    setLiveTick((tick) => tick + 1)
    const timeout = setTimeout(
      () => setLiveTick((tick) => tick + 1),
      AGENT_LIVE_WINDOW_MS - elapsed,
    )
    return () => clearTimeout(timeout)
  }, [lastAgentActionAt])
  const isLive = lastAgentActionAt > 0 && Date.now() - lastAgentActionAt < AGENT_LIVE_WINDOW_MS
  void liveTick

  useEffect(() => {
    if (!sessionId) return
    if (!isTauriRuntime()) return
    ensure(sessionId)
  }, [sessionId, ensure])

  useEffect(() => {
    if (!isTauriRuntime()) return
    return () => {
      useBrowserPanelStore.setState((state) => (
        state.activeSessionId
          ? { activeSessionId: null }
          : state
      ))
      dockHide().catch((err) => {
        console.warn('[browserDock] dockHide on panel unmount failed', err)
      })
    }
  }, [])

  const lastObservedSessionRef = useRef<string | null>(null)
  useEffect(() => {
    if (!isTauriRuntime()) return
    const prev = lastObservedSessionRef.current
    lastObservedSessionRef.current = sessionId
    if (!sessionId) {
      if (prev !== null) {
        dockHide().catch((err) => {
          console.warn('[browserDock] dockHide on tab leave failed', err)
        })
      }
      return
    }
    if (prev === sessionId) return
    void useBrowserPanelStore.getState().refreshTabs(sessionId)
    const store = useBrowserPanelStore.getState()
    const newPanel = store.panels[sessionId]
    if (!newPanel?.visible) {
      dockHide().catch((err) => {
        console.warn('[browserDock] dockHide on session switch failed', err)
      })
      return
    }
    if (store.activeSessionId !== sessionId) {
      useBrowserPanelStore.setState({ activeSessionId: sessionId })
    }
    const rect = newPanel.anchorRect
    if (!rect || rect.w < 100 || rect.h < 100) {
      dockPresentSession(sessionId).catch((err) => {
        console.warn('[browserDock] present_session on session switch failed', err)
      })
      return
    }
    void (async () => {
      try {
        await dockPresentSession(sessionId)
        await dockOpen(rect, null, sessionId)
      } catch (err) {
        console.warn('[browserDock] dock present/open on session switch failed', err)
      }
    })()
  }, [sessionId])

  const [agentBubblesBySession, setAgentBubblesBySession] = useState<
    Record<string, AgentBubble[]>
  >({})
  const [takeoverTabsBySession, setTakeoverTabsBySession] = useState<
    Record<string, Record<number, number>>
  >({})
  const unsubRef = useRef<(() => void) | null>(null)
  useEffect(() => {
    if (!isTauriRuntime()) return
    let cancelled = false
    listenDockEvents((event) => {
      if (cancelled) return
      ingestEvent(event)
      const sid =
        typeof (event as { sessionId?: string | null }).sessionId === 'string'
          ? ((event as { sessionId?: string }).sessionId as string)
          : null
      if (event.kind === 'agent_action') {
        if (!sid) return
        const data = event.data as { kind?: string; ts?: number }
        const ts = typeof data.ts === 'number' ? data.ts : Date.now()
        const id = ts + Math.random()
        const kind = typeof data.kind === 'string' ? data.kind : 'unknown'
        setAgentBubblesBySession((prev) => {
          const bucket = prev[sid] ?? []
          return {
            ...prev,
            [sid]: [...bucket.slice(-3), { id, kind, ts }],
          }
        })
        setTimeout(() => {
          setAgentBubblesBySession((prev) => {
            const bucket = prev[sid]
            if (!bucket) return prev
            const next = bucket.filter((b) => b.id !== id)
            if (next.length === bucket.length) return prev
            return next.length
              ? { ...prev, [sid]: next }
              : (() => {
                  const copy = { ...prev }
                  delete copy[sid]
                  return copy
                })()
          })
        }, AGENT_BUBBLE_WINDOW_MS)
      } else if (event.kind === 'dock_takeover') {
        const data = event.data as {
          tab_id?: number
          started_at?: number
          sessionId?: string | null
        }
        const tabId = typeof data.tab_id === 'number' ? data.tab_id : null
        const eventSid =
          sid ??
          (typeof data.sessionId === 'string' && data.sessionId ? data.sessionId : null)
        if (tabId !== null && eventSid) {
          const startedAt =
            typeof data.started_at === 'number' ? data.started_at : Date.now()
          setTakeoverTabsBySession((prev) => ({
            ...prev,
            [eventSid]: { ...(prev[eventSid] ?? {}), [tabId]: startedAt },
          }))
        }
      } else if (event.kind === 'dock_takeover_end') {
        const data = event.data as { tab_id?: number; sessionId?: string | null }
        const tabId = typeof data.tab_id === 'number' ? data.tab_id : null
        const eventSid =
          sid ??
          (typeof data.sessionId === 'string' && data.sessionId ? data.sessionId : null)
        if (tabId !== null && eventSid) {
          setTakeoverTabsBySession((prev) => {
            const bucket = prev[eventSid]
            if (!bucket || !(tabId in bucket)) return prev
            const nextBucket = { ...bucket }
            delete nextBucket[tabId]
            if (Object.keys(nextBucket).length === 0) {
              const copy = { ...prev }
              delete copy[eventSid]
              return copy
            }
            return { ...prev, [eventSid]: nextBucket }
          })
        }
      }
    })
      .then((unlisten) => {
        if (cancelled) {
          try {
            unlisten()
          } catch {

          }
          return
        }
        unsubRef.current = unlisten
      })
      .catch((err) => {
        console.warn('[browserDock] listenDockEvents subscription failed', err)
      })
    return () => {
      cancelled = true
      if (unsubRef.current) {
        try {
          unsubRef.current()
        } catch {

        }
        unsubRef.current = null
      }
    }
  }, [ingestEvent])

  const agentBubbles = sessionId ? agentBubblesBySession[sessionId] ?? [] : []
  const takeoverTabs = sessionId ? takeoverTabsBySession[sessionId] ?? {} : {}

  const setDrawerHeightRatio = useBrowserPanelStore((s) => s.setDrawerHeightRatio)

  const viewportRef = useRef<HTMLDivElement>(null)
  const panelShellRef = useRef<HTMLDivElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const measureRef = useRef<() => void>(() => {})
  const dragModeRef = useRef(false)
  const lateIdsRef = useRef<number[]>([])
  const [dragSnapshot, setDragSnapshot] = useState<string | null>(null)
  const [shellHeightPx, setShellHeightPx] = useState(0)
  useEffect(() => {
    const el = panelShellRef.current
    if (!el) return
    const ro = new ResizeObserver(() => {
      setShellHeightPx(el.clientHeight)
    })
    ro.observe(el)
    setShellHeightPx(el.clientHeight)
    return () => ro.disconnect()
  }, [visible])

  const sidebarOpen = useUIStore((s) => s.sidebarOpen)
  const rightSidebarOpen = useUIStore((s) => s.rightSidebarOpen)
  const rightSidebarWidth = useUIStore((s) => s.rightSidebarWidth)
  const columnWidth = panel?.columnWidth ?? BROWSER_COLUMN_WIDTH_BOUNDS.default

  useEffect(() => {
    if (!sessionId) return
    if (!isTauriRuntime()) return
    const el = viewportRef.current
    if (!el) return

    let lastRectSig = ''
    let lastSafeRect: { x: number; y: number; w: number; h: number } | null = null
    let scheduled = false
    let debounceTimer: number | null = null

    const computeRect = () => {
      const rect = el.getBoundingClientRect()
      if (rect.width <= 0 || rect.height <= 0) return null
      const host = el.parentElement?.getBoundingClientRect() ?? null
      const clamped = clampRectToHost(
        { x: rect.left, y: rect.top, w: rect.width, h: rect.height },
        host
          ? {
              left: host.left,
              top: host.top,
              right: host.right,
              bottom: host.bottom,
            }
          : null,
      )
      return {
        x: clamped.x,
        y: clamped.y,
        w: Math.max(1, clamped.w - 1),
        h: Math.max(1, clamped.h - 1),
      }
    }

    const measureNow = () => {
      const safe = computeRect()
      if (!safe) return
      lastSafeRect = safe
      if (dragModeRef.current) return
      const sig = `${Math.round(safe.x)}|${Math.round(safe.y)}|${Math.round(safe.w)}|${Math.round(safe.h)}`
      if (sig === lastRectSig) return
      lastRectSig = sig
      setAnchorRect(sessionId, safe)
      dockSetRect(safe).catch((err) => {
        console.warn('[browserDock] dockSetRect failed', err)
      })
    }

    const measure = () => {
      if (scheduled) return
      scheduled = true
      requestAnimationFrame(() => {
        scheduled = false
        if (debounceTimer !== null) window.clearTimeout(debounceTimer)
        debounceTimer = window.setTimeout(() => {
          debounceTimer = null
          measureNow()
        }, 50)
      })
    }

    const resync = () => {
      const safe = computeRect()
      if (!safe) return
      lastSafeRect = safe
      const sig = `${Math.round(safe.x)}|${Math.round(safe.y)}|${Math.round(safe.w)}|${Math.round(safe.h)}`
      lastRectSig = sig
      setAnchorRect(sessionId, safe)
      dockResync(safe).catch((err) => {
        console.warn('[browserDock] resync failed', err)
      })
    }

    measureRef.current = measureNow
    measureNow()

    const ro = new ResizeObserver(() => measure())
    ro.observe(el)
    if (panelShellRef.current) ro.observe(panelShellRef.current)
    if (containerRef.current) ro.observe(containerRef.current)
    window.addEventListener('resize', measure)

    const remeasureHandler = () => measure()
    const resyncHandler = () => resync()
    document.addEventListener('browser-panel-remeasure', remeasureHandler)
    document.addEventListener('browser-panel-resync', resyncHandler)

    return () => {
      if (debounceTimer !== null) window.clearTimeout(debounceTimer)
      ro.disconnect()
      window.removeEventListener('resize', measure)
      document.removeEventListener('browser-panel-remeasure', remeasureHandler)
      document.removeEventListener('browser-panel-resync', resyncHandler)
      measureRef.current = () => {}
      void lastSafeRect
    }
  }, [sessionId, setAnchorRect, visible])

  useEffect(() => {
    measureRef.current()
  }, [
    consoleOpen,
    inspectorOpen,
    driverOpen,
    drawerHeightRatio,
    rightSidebarOpen,
    rightSidebarWidth,
    columnWidth,
    columnWidthAuto,
  ])

  useEffect(() => {
    measureRef.current()
    const ids = [60, 180, 360, 560].map((ms) =>
      window.setTimeout(() => measureRef.current(), ms),
    )
    return () => {
      for (const id of ids) window.clearTimeout(id)
    }
  }, [sidebarOpen])

  useEffect(() => {
    if (!isTauriRuntime()) return
    if (!sessionId) return

    let snapshotToken = 0
    let restoreTimer: number | null = null

    const onStart = () => {
      const stateNow = useBrowserPanelStore.getState()
      const panelNow = stateNow.panels[sessionId]
      const owns = stateNow.activeSessionId === sessionId
      const visibleNow = panelNow?.visible ?? false
      const hasContentNow = Boolean(
        (panelNow?.liveUrl && panelNow.liveUrl.trim()) ||
          (panelNow?.url && panelNow.url.trim()),
      )
      dragModeRef.current = true
      if (!owns || !visibleNow || !hasContentNow) return
      const myToken = ++snapshotToken
      if (restoreTimer !== null) {
        window.clearTimeout(restoreTimer)
        restoreTimer = null
      }
      void (async () => {
        try {
          const result = await dockScreenshot(false)
          if (myToken !== snapshotToken) return
          if (result?.png_base64) {
            setDragSnapshot(`data:image/png;base64,${result.png_base64}`)
          }
        } catch (err) {
          console.warn('[browserDock] drag snapshot failed', err)
        } finally {
          if (myToken === snapshotToken) {
            dockPark().catch((err) => {
              console.warn('[browserDock] dockPark failed', err)
            })
          }
        }
      })()
    }

    const onEnd = () => {
      const stateNow = useBrowserPanelStore.getState()
      const panelNow = stateNow.panels[sessionId]
      const owns = stateNow.activeSessionId === sessionId
      const visibleNow = panelNow?.visible ?? false
      const hasContentNow = Boolean(
        (panelNow?.liveUrl && panelNow.liveUrl.trim()) ||
          (panelNow?.url && panelNow.url.trim()),
      )
      dragModeRef.current = false
      snapshotToken += 1
      for (const id of lateIdsRef.current) window.clearTimeout(id)
      lateIdsRef.current = []
      measureRef.current()
      window.requestAnimationFrame(() => {
        measureRef.current()
        document.dispatchEvent(new CustomEvent('browser-panel-resync'))
      })
      lateIdsRef.current = [80, 200].map((ms) =>
        window.setTimeout(() => {
          measureRef.current()
          document.dispatchEvent(new CustomEvent('browser-panel-resync'))
        }, ms),
      )
      if (owns && visibleNow && hasContentNow) {
        if (restoreTimer !== null) window.clearTimeout(restoreTimer)
        restoreTimer = window.setTimeout(() => {
          setDragSnapshot(null)
          restoreTimer = null
        }, 220)
      } else {
        setDragSnapshot(null)
      }
    }

    document.addEventListener('browser-panel-drag-start', onStart)
    document.addEventListener('browser-panel-drag-end', onEnd)
    return () => {
      document.removeEventListener('browser-panel-drag-start', onStart)
      document.removeEventListener('browser-panel-drag-end', onEnd)
      if (restoreTimer !== null) window.clearTimeout(restoreTimer)
      for (const id of lateIdsRef.current) window.clearTimeout(id)
      lateIdsRef.current = []
      dragModeRef.current = false
    }
  }, [sessionId])

  const tabs = panel?.tabs ?? []
  const activeBrowserTabId = panel?.activeTabId ?? null

  const navigate = useBrowserPanelStore((s) => s.navigate)
  const back = useBrowserPanelStore((s) => s.back)
  const forward = useBrowserPanelStore((s) => s.forward)
  const reload = useBrowserPanelStore((s) => s.reload)
  const zoomAction = useBrowserPanelStore((s) => s.zoom)
  const togglePick = useBrowserPanelStore((s) => s.togglePick)
  const toggleConsole = useBrowserPanelStore((s) => s.toggleConsole)
  const toggleInspector = useBrowserPanelStore((s) => s.toggleInspector)
  const clearStorage = useBrowserPanelStore((s) => s.clearStorage)
  const closeForSession = useBrowserPanelStore((s) => s.closeForSession)
  const clearConsole = useBrowserPanelStore((s) => s.clearConsole)
  const toggleDriver = useBrowserPanelStore((s) => s.toggleDriver)
  const clearAgentLog = useBrowserPanelStore((s) => s.clearAgentLog)
  const clearUserLog = useBrowserPanelStore((s) => s.clearUserLog)
  const newTabAction = useBrowserPanelStore((s) => s.newTab)
  const closeTabAction = useBrowserPanelStore((s) => s.closeTab)
  const activateTabAction = useBrowserPanelStore((s) => s.activateTab)
  const refreshTabs = useBrowserPanelStore((s) => s.refreshTabs)

  useEffect(() => {
    if (!sessionId || !visible) return
    void refreshTabs(sessionId)
  }, [sessionId, visible, refreshTabs])

  useEffect(() => {
    if (!isTauriRuntime()) return
    if (!sessionId) return
    if (!visible) return
    if (!ownsDock) return
    if (!hasContent) {
      dockHide().catch((err) => {
        console.warn('[browserDock] dockHide on empty content failed', err)
      })
      return
    }
    const current = useBrowserPanelStore.getState().panels[sessionId]
    const rect = current?.anchorRect
    if (!rect || rect.w < 100 || rect.h < 100) {
      return
    }
    void (async () => {
      try {
        await dockPresentSession(sessionId)
        await dockOpen(rect, null, sessionId)
      } catch (err) {
        console.warn('[browserDock] dock present/open on visibility change failed', err)
      }
    })()
  }, [sessionId, visible, ownsDock, hasContent])

  const [draftUrl, setDraftUrl] = useState(url)
  const urlInputRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (
      urlInputRef.current &&
      document.activeElement === urlInputRef.current
    ) {
      return
    }
    setDraftUrl(url)
  }, [url])

  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!menuOpen) return
    const close = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [menuOpen])

  useEffect(() => {
    if (!sessionId) return
    if (!pickMode) return
    const handler = (e: PointerEvent) => {
      const target = e.target as HTMLElement | null
      if (!target) return
      if (target.closest('[data-browser-viewport="true"]')) return
      if (target.closest('[data-browser-pick-toggle="true"]')) return
      void togglePick(sessionId)
    }
    document.addEventListener('pointerdown', handler, true)
    return () => document.removeEventListener('pointerdown', handler, true)
  }, [pickMode, sessionId, togglePick])

  const handleNavigate = useCallback(() => {
    if (!sessionId) return
    void navigate(sessionId, draftUrl)
  }, [sessionId, draftUrl, navigate])

  const handleCopyUrl = useCallback(async () => {
    const target = liveUrl || url
    if (!target) return
    try {
      await navigator.clipboard.writeText(target)
    } catch (err) {
      console.warn('[browserDock] copy URL failed', err)
    }
    setMenuOpen(false)
  }, [liveUrl, url])

  const addToast = useUIStore((s) => s.addToast)

  const triggerScreenshot = useCallback(
    async (fullPage: boolean) => {
      if (!sessionId) return
      try {
        const result = await dockScreenshot(fullPage)
        if (!result || !result.png_base64) {
          addToast({
            type: 'warning',
            message: t('browser.panel.toast.screenshotFailed'),
          })
          return
        }
        const dataUrl = `data:image/png;base64,${result.png_base64}`
        const ts = new Date().toISOString().replace(/[:.]/g, '-')
        const filename = `browser-screenshot-${ts}.png`
        try {
          const a = document.createElement('a')
          a.href = dataUrl
          a.download = filename
          document.body.appendChild(a)
          a.click()
          document.body.removeChild(a)
          addToast({
            type: 'success',
            message: t('browser.panel.toast.screenshotSaved'),
          })
        } catch (err) {
          console.warn('[browserDock] screenshot download failed', err)
          try {
            await navigator.clipboard.writeText(dataUrl)
            addToast({
              type: 'success',
              message: t('browser.panel.toast.screenshotCopied'),
            })
          } catch (clipErr) {
            console.warn('[browserDock] screenshot clipboard fallback failed', clipErr)
            addToast({
              type: 'error',
              message: t('browser.panel.toast.screenshotFailed'),
            })
          }
        }
      } catch (err) {
        console.warn('[browserDock] screenshot failed', err)
        addToast({
          type: 'error',
          message: `${t('browser.panel.toast.screenshotFailed')}: ${
            err instanceof Error ? err.message : String(err)
          }`,
        })
      }
    },
    [sessionId, addToast, t],
  )

  const handleScreenshot = useCallback(() => {
    setMenuOpen(false)
    void triggerScreenshot(false)
  }, [triggerScreenshot])

  const handleAreaScreenshot = useCallback(() => {
    setMenuOpen(false)
    void triggerScreenshot(true)
  }, [triggerScreenshot])

  const splitterDrawerHandler = useCallback(
    (deltaPx: number) => {
      if (!sessionId) return
      const totalH = shellHeightPx > 0 ? shellHeightPx : panelShellRef.current?.clientHeight ?? 480
      if (totalH <= 0) return
      const next = drawerHeightRatio - deltaPx / totalH
      setDrawerHeightRatio(sessionId, next)
    },
    [sessionId, drawerHeightRatio, setDrawerHeightRatio, shellHeightPx],
  )

  if (!sessionId) return null
  if (isMemberSession) return null
  if (!isTauriRuntime()) return null
  if (!visible) return null

  const headerLabel = title || liveUrl || url || t('browser.panel.title')
  const onSubmitUrl = (e: React.FormEvent) => {
    e.preventDefault()
    handleNavigate()
  }

  const drawerVisible = consoleOpen || inspectorOpen || driverOpen
  const drawerCount = (consoleOpen ? 1 : 0) + (inspectorOpen ? 1 : 0) + (driverOpen ? 1 : 0)

  return (
    <aside
      ref={containerRef}
      data-testid="embedded-browser-panel"
      data-width-mode={columnWidthAuto ? 'auto' : 'manual'}
      className={
        columnWidthAuto
          ? 'flex h-full min-h-0 min-w-[240px] flex-1 flex-col overflow-hidden border-l border-[var(--color-border)] bg-[var(--color-surface-container-low)]'
          : 'flex h-full min-h-0 min-w-[240px] flex-col overflow-hidden border-l border-[var(--color-border)] bg-[var(--color-surface-container-low)]'
      }
      style={
        columnWidthAuto
          ? undefined
          : {
              flex: `0 1 ${panel?.columnWidth ?? BROWSER_COLUMN_WIDTH_BOUNDS.default}px`,
              maxWidth: '100%',
            }
      }
    >
      <div
        ref={panelShellRef}
        className={`flex h-full min-h-0 flex-col overflow-hidden ${
          isLive ? 'animate-browser-dock-pulse' : ''
        }`}
      >
        {(() => {
          const agentBusy =
            chatState === 'tool_executing' && activeToolName === 'browser'
          if (!agentBusy) return null
          return (
            <div
              className="pointer-events-none flex items-center gap-1.5 border-b border-[var(--color-brand)]/40 bg-[var(--color-brand)]/12 px-3 py-1 text-[10px] font-medium text-[var(--color-brand)]"
              style={{ animation: 'browser-dock-pulse 1.6s ease-in-out infinite' }}
            >
              <span
                className="inline-block h-1.5 w-1.5 rounded-full bg-[var(--color-brand)]"
                aria-hidden="true"
              />
              <span>
                {t('debug.qa.dock.agentBusy', {
                  tabId: activeBrowserTabId ?? '-',
                })}
              </span>
            </div>
          )
        })()}
        <div className="relative flex shrink-0 items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5"
          style={{ minHeight: HEADER_PX }}
        >
          <div className="flex min-w-0 items-center gap-2">
            <span
              className="material-symbols-outlined text-[16px] text-[var(--color-brand)]"
              aria-hidden="true"
            >
              public
            </span>
            {isLive && (
              <span
                title={t('browser.panel.driver.live')}
                className="inline-flex items-center gap-1 rounded-full bg-[var(--color-brand)]/12 px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-brand)]"
              >
                <span
                  className="inline-block h-1.5 w-1.5 rounded-full bg-[var(--color-brand)] animate-pulse-dot"
                />
                {t('browser.panel.driver.live')}
              </span>
            )}
            <span className="truncate text-[12px] font-medium text-[var(--color-text-primary)]">
              {headerLabel}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => sessionId && void closeForSession(sessionId)}
              className="inline-flex h-6 w-6 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-error)]"
              title={t('browser.panel.close')}
              aria-label={t('browser.panel.close')}
            >
              <span className="material-symbols-outlined text-[14px]">close</span>
            </button>
          </div>
        </div>

        <>
            {tabs.length > 0 && (
              <div
                className="flex shrink-0 items-end gap-1 overflow-x-auto border-b border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] px-2 pt-1.5"
                style={{ minHeight: TABBAR_PX }}
              >
                {tabs.map((tab) => {
                  const isActive = tab.id === activeBrowserTabId
                  const label = tab.title || tab.url || t('browser.panel.tabs.untitled')
                  const tabAgentTs = panel?.tabActivity?.[tab.id] ?? 0
                  const tabIsLive =
                    tabAgentTs > 0 && Date.now() - tabAgentTs < AGENT_LIVE_WINDOW_MS
                  const ownedByAgent = tab.owner === 'agent'
                  const inTakeover = !!takeoverTabs[tab.id]
                  const isPinned = (panel?.preferredTestTabId ?? null) === tab.id
                  void liveTick
                  return (
                    <div
                      key={tab.id}
                      role="tab"
                      aria-selected={isActive}
                      onClick={() => sessionId && void activateTabAction(sessionId, tab.id)}
                      title={
                        inTakeover
                          ? t('debug.qa.takeover')
                          : tabIsLive
                            ? t('browser.panel.tabs.agentActive')
                            : isPinned
                              ? t('browser.panel.tabs.testTargetBadge')
                              : t('browser.panel.tabs.activate')
                      }
                      className={`group flex h-7 max-w-[220px] cursor-pointer items-center gap-1 rounded-t-md border border-b-0 px-2 text-[12px] transition-colors ${
                        inTakeover
                          ? 'border-[var(--color-error)] bg-[var(--color-surface)] text-[var(--color-text-primary)] ring-1 ring-[var(--color-error)]'
                          : isPinned
                            ? 'border-[var(--color-brand)] bg-[var(--color-surface)] text-[var(--color-text-primary)] ring-1 ring-[var(--color-brand)]'
                            : isActive
                              ? 'border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-primary)]'
                              : 'border-transparent bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                      }`}
                      style={
                        inTakeover
                          ? { animation: 'browser-dock-pulse 1.2s ease-in-out infinite' }
                          : undefined
                      }
                    >
                      {inTakeover ? (
                        <span
                          aria-label={t('debug.qa.takeover')}
                          className="material-symbols-outlined text-[14px] text-[var(--color-error)]"
                        >
                          radar
                        </span>
                      ) : tabIsLive ? (
                        <span
                          aria-hidden="true"
                          className="inline-block h-1.5 w-1.5 rounded-full bg-[var(--color-brand)]"
                          style={{ animation: 'browser-dock-pulse 1.5s ease-in-out infinite' }}
                        />
                      ) : (
                        <span
                          className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]"
                          aria-hidden="true"
                        >
                          {ownedByAgent ? 'smart_toy' : 'public'}
                        </span>
                      )}
                      <span className="truncate">{label}</span>
                      {isPinned && (
                        <span
                          aria-label={t('browser.panel.tabs.testTargetBadge')}
                          title={t('browser.panel.tabs.testTargetBadge')}
                          className="ml-0.5 inline-flex items-center rounded-sm border border-[var(--color-brand)] px-1 py-px text-[10px] font-semibold uppercase tracking-wide text-[var(--color-brand)]"
                        >
                          QA
                        </span>
                      )}
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation()
                          if (!sessionId) return
                          if (isPinned) {
                            unbindDebugTab(sessionId, tab.id)
                          } else {
                            bindDebugTab(sessionId, tab.id)
                          }
                        }}
                        title={
                          isPinned
                            ? t('browser.panel.tabs.unpinTestTarget')
                            : t('browser.panel.tabs.pinAsTestTarget')
                        }
                        aria-label={
                          isPinned
                            ? t('browser.panel.tabs.unpinTestTarget')
                            : t('browser.panel.tabs.pinAsTestTarget')
                        }
                        className={`ml-1 inline-flex h-4 w-4 items-center justify-center rounded transition-colors ${
                          isPinned
                            ? 'text-[var(--color-brand)] hover:bg-[var(--color-surface-hover)]'
                            : 'text-[var(--color-text-tertiary)] opacity-60 hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-brand)] hover:opacity-100'
                        }`}
                      >
                        <span className="material-symbols-outlined text-[12px]">push_pin</span>
                      </button>
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation()
                          if (sessionId) void closeTabAction(sessionId, tab.id)
                        }}
                        title={t('browser.panel.tabs.close')}
                        aria-label={t('browser.panel.tabs.close')}
                        className="ml-1 inline-flex h-4 w-4 items-center justify-center rounded text-[var(--color-text-tertiary)] opacity-60 hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-error)] hover:opacity-100"
                      >
                        <span className="material-symbols-outlined text-[12px]">close</span>
                      </button>
                    </div>
                  )
                })}
                <button
                  type="button"
                  onClick={() => sessionId && void newTabAction(sessionId, null, true)}
                  title={t('browser.panel.tabs.new')}
                  aria-label={t('browser.panel.tabs.new')}
                  className="ml-0.5 inline-flex h-7 w-7 items-center justify-center rounded-t-md text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
                >
                  <span className="material-symbols-outlined text-[16px]">add</span>
                </button>
              </div>
            )}

            <div
              className="flex w-full min-w-0 shrink-0 items-center gap-1 overflow-hidden border-b border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] px-2 py-1.5"
              style={{ minHeight: TOOLBAR_PX }}
            >
              <NavBtn icon="arrow_back" title={t('browser.panel.back')} onClick={() => sessionId && void back(sessionId)} />
              <NavBtn icon="arrow_forward" title={t('browser.panel.forward')} onClick={() => sessionId && void forward(sessionId)} />
              <NavBtn
                icon="refresh"
                title={t('browser.panel.reload')}
                onClick={() => sessionId && void reload(sessionId, false)}
              />
              <form onSubmit={onSubmitUrl} className="flex min-w-0 flex-1 items-center">
                <input
                  ref={urlInputRef}
                  type="text"
                  value={draftUrl}
                  onChange={(e) => setDraftUrl(e.target.value)}
                  onBlur={() => {
                    if (draftUrl !== url) setDraftUrl(url)
                  }}
                  placeholder={t('browser.panel.urlPlaceholder')}
                  className="h-7 w-full min-w-0 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
                  spellCheck={false}
                  autoCapitalize="off"
                  autoCorrect="off"
                />
              </form>
              <div className="ml-1 flex shrink-0 items-center gap-1">
                <ToolbarToggleBtn
                  icon="ads_click"
                  title={t('browser.panel.pickElement')}
                  active={pickMode}
                  onClick={() => sessionId && void togglePick(sessionId)}
                  dataAttrs={{ 'data-browser-pick-toggle': 'true' }}
                />
                <ToolbarToggleBtn
                  icon="terminal"
                  title={consoleOpen ? t('browser.panel.consoleHide') : t('browser.panel.consoleShow')}
                  active={consoleOpen}
                  onClick={() => sessionId && void toggleConsole(sessionId)}
                />
                <ToolbarToggleBtn
                  icon="format_paint"
                  title={inspectorOpen ? t('browser.panel.inspectorHide') : t('browser.panel.inspectorShow')}
                  active={inspectorOpen}
                  onClick={() => sessionId && void toggleInspector(sessionId)}
                />
                <ToolbarToggleBtn
                  icon="smart_toy"
                  title={driverOpen ? t('browser.panel.driver.hide') : t('browser.panel.driver.show')}
                  active={driverOpen}
                  onClick={() => sessionId && toggleDriver(sessionId)}
                />
                <div className="relative" ref={menuRef}>
                  <ToolbarToggleBtn
                    icon="more_vert"
                    title={t('browser.panel.more')}
                    active={menuOpen}
                    onClick={() => setMenuOpen((v) => !v)}
                  />
                  {menuOpen && (
                    <div
                      role="menu"
                      className="absolute right-0 top-full z-30 mt-1 w-[224px] overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]"
                    >
                      <MenuItem
                        icon="photo_camera"
                        label={t('browser.panel.menu.screenshot')}
                        onClick={handleScreenshot}
                      />
                      <MenuItem
                        icon="crop"
                        label={t('browser.panel.menu.areaScreenshot')}
                        onClick={handleAreaScreenshot}
                      />
                      <MenuItem
                        icon="autorenew"
                        label={t('browser.panel.menu.hardReload')}
                        onClick={() => {
                          if (sessionId) void reload(sessionId, true)
                          setMenuOpen(false)
                        }}
                      />
                      <MenuItem
                        icon="content_copy"
                        label={t('browser.panel.menu.copyUrl')}
                        onClick={handleCopyUrl}
                      />
                      <div className="flex items-center gap-1 border-t border-[var(--color-border)] px-2 py-1">
                        <span className="mr-auto text-[11px] text-[var(--color-text-tertiary)]">
                          {t('browser.panel.menu.zoom')} {Math.round(clampZoom(zoom) * 100)}%
                        </span>
                        <button
                          type="button"
                          className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                          onClick={() => sessionId && void zoomAction(sessionId, -0.1)}
                          title={t('browser.panel.menu.zoomOut')}
                          aria-label={t('browser.panel.menu.zoomOut')}
                        >
                          <span className="material-symbols-outlined text-[14px]">remove</span>
                        </button>
                        <button
                          type="button"
                          className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                          onClick={() => sessionId && void zoomAction(sessionId, 'reset')}
                          title={t('browser.panel.menu.zoomReset')}
                          aria-label={t('browser.panel.menu.zoomReset')}
                        >
                          <span className="material-symbols-outlined text-[14px]">restart_alt</span>
                        </button>
                        <button
                          type="button"
                          className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                          onClick={() => sessionId && void zoomAction(sessionId, 0.1)}
                          title={t('browser.panel.menu.zoomIn')}
                          aria-label={t('browser.panel.menu.zoomIn')}
                        >
                          <span className="material-symbols-outlined text-[14px]">add</span>
                        </button>
                      </div>
                      <MenuItem
                        icon="history_toggle_off"
                        label={t('browser.panel.menu.clearHistory')}
                        onClick={() => {
                          if (sessionId) void clearStorage(sessionId, { history: true })
                          setMenuOpen(false)
                        }}
                      />
                      <MenuItem
                        icon="cookie"
                        label={t('browser.panel.menu.clearCookies')}
                        onClick={() => {
                          if (sessionId) void clearStorage(sessionId, { cookies: true })
                          setMenuOpen(false)
                        }}
                      />
                      <MenuItem
                        icon="delete_sweep"
                        label={t('browser.panel.menu.clearCache')}
                        onClick={() => {
                          if (sessionId) void clearStorage(sessionId, { cache: true })
                          setMenuOpen(false)
                        }}
                      />
                    </div>
                  )}
                </div>
              </div>
            </div>

            <div className="relative flex min-h-0 flex-1 flex-col">
              <div
                ref={viewportRef}
                data-testid="embedded-browser-viewport"
                data-browser-viewport="true"
                className="relative w-full overflow-hidden bg-[var(--color-surface)]"
                style={{
                  flex: '1 1 0',
                  minHeight: VIEWPORT_MIN_PX,
                }}
                onMouseEnter={() => {
                  if (!isTauriRuntime() || !ownsDock || !hasContent) return
                  dockFocusActive().catch(() => {})
                }}
                onPointerDown={() => {
                  if (!isTauriRuntime() || !ownsDock || !hasContent) return
                  dockFocusActive().catch(() => {})
                }}
              >
                {dragSnapshot && (
                  <img
                    src={dragSnapshot}
                    alt=""
                    aria-hidden="true"
                    draggable={false}
                    className="pointer-events-none absolute inset-0 h-full w-full select-none"
                    style={{ objectFit: 'fill', imageRendering: 'auto' }}
                  />
                )}
                {!hasContent && (
                  <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 px-6 text-center select-none">
                    <span
                      className="material-symbols-outlined text-[48px] text-[var(--color-text-tertiary)]"
                      aria-hidden="true"
                    >
                      public
                    </span>
                    <div className="text-[14px] font-medium text-[var(--color-text-primary)]">
                      {t('browser.panel.empty.title')}
                    </div>
                    <div className="max-w-[320px] text-[12px] leading-relaxed text-[var(--color-text-tertiary)]">
                      {t('browser.panel.empty.hint')}
                    </div>
                  </div>
                )}
                {hasContent && !ownsDock && (
                  <div className="absolute inset-0 flex items-center justify-center text-[12px] text-[var(--color-text-tertiary)]">
                    {t('browser.panel.empty')}
                  </div>
                )}
                {agentBubbles.length > 0 && (
                  <div className="pointer-events-none absolute right-2 top-2 flex max-w-[60%] flex-col items-end gap-1">
                    {agentBubbles.map((b) => (
                      <span
                        key={b.id}
                        className="animate-browser-dock-bubble rounded-full bg-[var(--color-brand)]/95 px-2 py-0.5 text-[10px] font-medium text-white shadow"
                      >
                        {t('browser.panel.cooperate.agentBubble', { kind: b.kind })}
                      </span>
                    ))}
                  </div>
                )}
              </div>
              {drawerVisible && (
                <>
                  <BrowserPanelSplitter
                    orientation="horizontal"
                    onDrag={splitterDrawerHandler}
                    ariaLabel={t('browser.panel.splitter.dragHorizontal')}
                  />
                  <div
                    className="flex flex-col overflow-hidden border-t border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]"
                    style={{
                      flex: '0 0 auto',
                      minHeight: 80,
                      height: `${Math.max(
                        80,
                        Math.min(
                          (shellHeightPx > 0 ? shellHeightPx : 480) * 0.6,
                          drawerHeightRatio * (shellHeightPx > 0 ? shellHeightPx : 480),
                        ),
                      )}px`,
                      maxHeight: '60%',
                    }}
                  >
                    <div className="flex min-h-0 flex-1">
                      {consoleOpen && (
                        <div
                          className="flex min-w-0 flex-1 flex-col"
                          style={{
                            borderRightWidth: drawerCount > 1 ? 1 : 0,
                            borderRightStyle: 'solid',
                            borderRightColor: 'var(--color-border)',
                          }}
                        >
                          <ConsoleDrawer
                            entries={consoleLog}
                            onClear={() => sessionId && clearConsole(sessionId)}
                            title={t('browser.panel.consoleTitle')}
                            emptyLabel={t('browser.panel.consoleEmpty')}
                            clearLabel={t('browser.panel.consoleClear')}
                          />
                        </div>
                      )}
                      {inspectorOpen && (
                        <div
                          className="flex min-w-0 flex-1 flex-col"
                          style={{
                            borderRightWidth: driverOpen ? 1 : 0,
                            borderRightStyle: 'solid',
                            borderRightColor: 'var(--color-border)',
                          }}
                        >
                          <InspectorDrawer
                            snapshot={inspector}
                            emptyLabel={t('browser.panel.inspectorEmpty')}
                            title={t('browser.panel.inspectorTitle')}
                          />
                        </div>
                      )}
                      {driverOpen && (
                        <div className="flex min-w-0 flex-1 flex-col">
                          <CooperateTimeline
                            agentEntries={agentLog}
                            userEntries={userLog}
                            onClear={() => {
                              if (!sessionId) return
                              clearAgentLog(sessionId)
                              clearUserLog(sessionId)
                            }}
                            title={t('browser.panel.cooperate.title')}
                            emptyLabel={t('browser.panel.cooperate.empty')}
                            clearLabel={t('browser.panel.driver.clear')}
                            agentLabel={t('browser.panel.actor.agent')}
                            userLabel={t('browser.panel.actor.user')}
                          />
                        </div>
                      )}
                    </div>
                  </div>
                </>
              )}
            </div>
          </>
      </div>
    </aside>
  )
}

function NavBtn({ icon, title, onClick }: { icon: string; title: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
    >
      <span className="material-symbols-outlined text-[16px]">{icon}</span>
    </button>
  )
}

function ToolbarToggleBtn({
  icon,
  title,
  active,
  onClick,
  dataAttrs,
}: {
  icon: string
  title: string
  active?: boolean
  onClick: () => void
  dataAttrs?: Record<string, string>
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      aria-pressed={active}
      {...(dataAttrs ?? {})}
      className={`inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-colors ${
        active
          ? 'bg-[var(--color-brand)]/12 text-[var(--color-brand)]'
          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
      }`}
    >
      <span className="material-symbols-outlined text-[16px]">{icon}</span>
    </button>
  )
}

function MenuItem({ icon, label, onClick }: { icon: string; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] text-[var(--color-text-primary)] transition-colors hover:bg-[var(--color-surface-hover)]"
    >
      <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">{icon}</span>
      <span>{label}</span>
    </button>
  )
}

function ConsoleDrawer({
  entries,
  onClear,
  title,
  emptyLabel,
  clearLabel,
}: {
  entries: BrowserConsoleEntry[]
  onClear: () => void
  title: string
  emptyLabel: string
  clearLabel: string
}) {
  const scrollRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [entries.length])

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between px-3 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]">
        <span>{title}</span>
        <button
          type="button"
          onClick={onClear}
          className="rounded px-2 py-0.5 text-[11px] hover:bg-[var(--color-surface-hover)]"
        >
          {clearLabel}
        </button>
      </div>
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto px-3 pb-1 font-mono text-[11px]">
        {entries.length === 0 ? (
          <div className="py-2 text-[var(--color-text-tertiary)]">{emptyLabel}</div>
        ) : (
          entries.map((entry) => (
            <div
              key={entry.id}
              className={`whitespace-pre-wrap break-words py-0.5 ${consoleColor(entry.level)}`}
            >
              <span className="mr-2 opacity-60">{new Date(entry.ts).toLocaleTimeString()}</span>
              <span className="mr-2 font-semibold uppercase opacity-70">{entry.level}</span>
              <span>{entry.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}

function consoleColor(level: string): string {
  switch (level) {
    case 'error':
      return 'text-[var(--color-error)]'
    case 'warn':
      return 'text-[var(--color-warning)]'
    case 'info':
      return 'text-[var(--color-info)]'
    case 'debug':
      return 'text-[var(--color-text-tertiary)]'
    default:
      return 'text-[var(--color-text-primary)]'
  }
}

function InspectorDrawer({
  snapshot,
  emptyLabel,
  title,
}: {
  snapshot: BrowserInspectorSnapshot | null
  emptyLabel: string
  title: string
}) {
  const rows = useMemo(() => {
    if (!snapshot) return [] as Array<[string, string]>
    return Object.entries(snapshot.props).filter(([k]) => k !== '__rect__')
  }, [snapshot])

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between px-3 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]">
        <span>{title}</span>
        {snapshot ? (
          <span className="truncate text-[10px] text-[var(--color-text-tertiary)]">{snapshot.selector}</span>
        ) : null}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-1 font-mono text-[11px]">
        {!snapshot ? (
          <div className="py-2 text-[var(--color-text-tertiary)]">{emptyLabel}</div>
        ) : (
          <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
            {rows.map(([k, v]) => (
              <span key={k} className="contents">
                <span className="text-[var(--color-text-tertiary)]">{k}</span>
                <span className="break-words text-[var(--color-text-primary)]">{v}</span>
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

type TimelineRow =
  | {
      ts: number
      key: string
      actor: 'agent'
      kind: string
      detail: string
    }
  | {
      ts: number
      key: string
      actor: 'user'
      kind: string
      detail: string
    }

function CooperateTimeline({
  agentEntries,
  userEntries,
  onClear,
  title,
  emptyLabel,
  clearLabel,
  agentLabel,
  userLabel,
}: {
  agentEntries: BrowserAgentActionEntry[]
  userEntries: BrowserUserActionEntry[]
  onClear: () => void
  title: string
  emptyLabel: string
  clearLabel: string
  agentLabel: string
  userLabel: string
}) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const rows = useMemo<TimelineRow[]>(() => {
    const merged: TimelineRow[] = []
    for (const e of agentEntries) {
      merged.push({
        ts: e.ts,
        key: `a-${e.id}`,
        actor: 'agent',
        kind: e.kind,
        detail: summarizeAgentArgs(e.args),
      })
    }
    for (const e of userEntries) {
      merged.push({
        ts: e.ts,
        key: `u-${e.id}`,
        actor: 'user',
        kind: e.kind,
        detail: e.detail,
      })
    }
    merged.sort((a, b) => a.ts - b.ts)
    return merged
  }, [agentEntries, userEntries])

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [rows.length])

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between px-3 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]">
        <span>{title}</span>
        <button
          type="button"
          onClick={onClear}
          className="rounded px-2 py-0.5 text-[11px] hover:bg-[var(--color-surface-hover)]"
        >
          {clearLabel}
        </button>
      </div>
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto px-3 pb-1 font-mono text-[11px]">
        {rows.length === 0 ? (
          <div className="py-2 text-[var(--color-text-tertiary)]">{emptyLabel}</div>
        ) : (
          rows.map((row) => (
            <div key={row.key} className="flex items-baseline gap-2 py-0.5">
              <span className="opacity-60">{new Date(row.ts).toLocaleTimeString()}</span>
              <span
                className={`rounded px-1.5 text-[10px] font-semibold uppercase ${
                  row.actor === 'agent'
                    ? 'bg-[var(--color-brand)]/12 text-[var(--color-brand)]'
                    : 'bg-[var(--color-info)]/12 text-[var(--color-info)]'
                }`}
              >
                {row.actor === 'agent' ? agentLabel : userLabel}
              </span>
              <span className="opacity-80">{row.kind}</span>
              <span className="break-all text-[var(--color-text-primary)]">{row.detail}</span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}

export const BROWSER_PANEL_HEIGHTS = {
  collapsed: HEADER_PX,
  toolbar: TOOLBAR_PX,
  tabbar: TABBAR_PX,
  viewportMin: VIEWPORT_MIN_PX,
} as const
