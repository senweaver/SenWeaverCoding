// SPDX-License-Identifier: MIT
//
// Embedded Browser dock chrome for the React shell.
//
// The panel is purely the UI chrome around a Tauri child WebView; the
// actual page rendering happens in the child webview owned by the Rust
// shell at `desktop/src-tauri/src/browser_dock.rs`.  A `ResizeObserver`
// watches the placeholder `<div ref={viewportRef}>` and forwards the
// viewport rect (in DPI-corrected logical pixels) to
// `browserPanelStore.setAnchorRect`, which calls
// `browser_dock_set_rect` so the OS-level webview tracks the React
// layout.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useTabStore } from '../../stores/tabStore'
import {
  type BrowserAgentActionEntry,
  type BrowserConsoleEntry,
  type BrowserInspectorSnapshot,
  useBrowserPanelStore,
} from '../../stores/browserPanelStore'
import { useTeamStore } from '../../stores/teamStore'
import { listenDockEvents } from '../../lib/browserDock'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { useTranslation } from '../../i18n'

const PANEL_HEIGHT_PX = 360
const PANEL_HEIGHT_COLLAPSED_PX = 38
const CONSOLE_DRAWER_HEIGHT_PX = 140
const INSPECTOR_DRAWER_HEIGHT_PX = 160
const DRIVER_DRAWER_HEIGHT_PX = 160
const AGENT_LIVE_WINDOW_MS = 5_000

function clampZoom(z: number): number {
  if (!Number.isFinite(z) || z <= 0) return 1
  return Math.min(3, Math.max(0.25, z))
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

  const visible = panel?.visible ?? false
  const expanded = panel?.expanded ?? false
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
  const lastAgentActionAt = panel?.lastAgentActionAt ?? 0
  const ownsDock = sessionId !== null && activeSessionId === sessionId

  const [liveTick, setLiveTick] = useState(0)
  useEffect(() => {
    if (!lastAgentActionAt) return
    const elapsed = Date.now() - lastAgentActionAt
    if (elapsed >= AGENT_LIVE_WINDOW_MS) return
    setLiveTick((t) => t + 1)
    const timeout = setTimeout(
      () => setLiveTick((t) => t + 1),
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

  const unsubRef = useRef<(() => void) | null>(null)
  useEffect(() => {
    if (!isTauriRuntime()) return
    let cancelled = false
    void listenDockEvents((event) => {
      if (cancelled) return
      ingestEvent(event)
    }).then((unlisten) => {
      if (cancelled) {
        try {
          unlisten()
        } catch {

        }
        return
      }
      unsubRef.current = unlisten
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

  const viewportRef = useRef<HTMLDivElement>(null)
  const panelShellRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!sessionId) return
    if (!isTauriRuntime()) return
    const el = viewportRef.current
    if (!el) return

    let rafId = 0
    const measure = () => {

      cancelAnimationFrame(rafId)
      rafId = requestAnimationFrame(() => {
        const rect = el.getBoundingClientRect()
        if (rect.width <= 0 || rect.height <= 0) return
        setAnchorRect(sessionId, {
          x: Math.round(rect.left),
          y: Math.round(rect.top),
          w: Math.round(rect.width),
          h: Math.round(rect.height),
        })
      })
    }
    measure()

    const ro = new ResizeObserver(() => measure())
    ro.observe(el)

    if (panelShellRef.current) ro.observe(panelShellRef.current)
    window.addEventListener('resize', measure)
    window.addEventListener('scroll', measure, true)
    return () => {
      cancelAnimationFrame(rafId)
      ro.disconnect()
      window.removeEventListener('resize', measure)
      window.removeEventListener('scroll', measure, true)
    }
  }, [sessionId, setAnchorRect, expanded, consoleOpen, inspectorOpen, driverOpen])

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
  const togglePanel = useBrowserPanelStore((s) => s.toggle)
  const clearConsole = useBrowserPanelStore((s) => s.clearConsole)
  const toggleDriver = useBrowserPanelStore((s) => s.toggleDriver)
  const clearAgentLog = useBrowserPanelStore((s) => s.clearAgentLog)
  const newTabAction = useBrowserPanelStore((s) => s.newTab)
  const closeTabAction = useBrowserPanelStore((s) => s.closeTab)
  const activateTabAction = useBrowserPanelStore((s) => s.activateTab)
  const refreshTabs = useBrowserPanelStore((s) => s.refreshTabs)

  useEffect(() => {
    if (!sessionId || !visible) return
    void refreshTabs(sessionId)
  }, [sessionId, visible, refreshTabs])

  const [draftUrl, setDraftUrl] = useState(url)
  useEffect(() => {
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

  const handleScreenshot = useCallback(() => {
    if (!sessionId) return
    if (!viewportRef.current) return
    const rect = viewportRef.current.getBoundingClientRect()
    const payload = {
      sessionId,
      area: { x: Math.round(rect.left), y: Math.round(rect.top), w: Math.round(rect.width), h: Math.round(rect.height) },
      area_capture: false,
    }
    window.dispatchEvent(new CustomEvent('browser-dock:screenshot', { detail: payload }))
    setMenuOpen(false)
  }, [sessionId])

  const handleAreaScreenshot = useCallback(() => {
    if (!sessionId) return
    if (!viewportRef.current) return
    const rect = viewportRef.current.getBoundingClientRect()
    const payload = {
      sessionId,
      area: { x: Math.round(rect.left), y: Math.round(rect.top), w: Math.round(rect.width), h: Math.round(rect.height) },
      area_capture: true,
    }
    window.dispatchEvent(new CustomEvent('browser-dock:screenshot', { detail: payload }))
    setMenuOpen(false)
  }, [sessionId])

  if (!sessionId) return null
  if (isMemberSession) return null
  if (!isTauriRuntime()) return null
  if (!visible) return null

  const headerLabel = title || liveUrl || url || t('browser.panel.title')

  const onSubmitUrl = (e: React.FormEvent) => {
    e.preventDefault()
    handleNavigate()
  }

  return (
    <div
      data-testid="embedded-browser-panel"
      className="pointer-events-none absolute inset-x-0 z-40 px-4 pb-2"
      style={{ paddingTop: 6, bottom: 'var(--composer-height, 0px)' }}
    >
      <div className="pointer-events-auto mx-auto w-full max-w-[860px]">
        <div
          ref={panelShellRef}
          className="overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] shadow-[var(--shadow-dropdown)]"
        >
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5">
          <div className="flex min-w-0 items-center gap-2">
            <span className="material-symbols-outlined text-[16px] text-[var(--color-brand)]" aria-hidden="true">
              public
            </span>
            {isLive && (
              <span
                title={t('browser.panel.driver.live')}
                className="inline-flex items-center gap-1 rounded-full bg-[var(--color-brand)]/12 px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-brand)]"
              >
                <span
                  className="inline-block h-1.5 w-1.5 rounded-full bg-[var(--color-brand)]"
                  style={{ animation: 'pulse 1.4s ease-in-out infinite' }}
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
              onClick={() => sessionId && void togglePanel(sessionId, { source: 'manual' })}
              className="inline-flex h-6 w-6 items-center justify-center rounded-md text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
              title={expanded ? t('browser.panel.collapse') : t('browser.panel.expand')}
              aria-label={expanded ? t('browser.panel.collapse') : t('browser.panel.expand')}
            >
              <span className="material-symbols-outlined text-[14px]">
                {expanded ? 'expand_more' : 'expand_less'}
              </span>
            </button>
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

        {expanded && (
          <>
            {}
            {tabs.length > 0 && (
              <div className="flex items-end gap-1 overflow-x-auto border-b border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] px-2 pt-1.5">
                {tabs.map((tab) => {
                  const isActive = tab.id === activeBrowserTabId
                  const label = tab.title || tab.url || t('browser.panel.tabs.untitled')
                  return (
                    <div
                      key={tab.id}
                      role="tab"
                      aria-selected={isActive}
                      onClick={() => sessionId && void activateTabAction(sessionId, tab.id)}
                      title={t('browser.panel.tabs.activate')}
                      className={`group flex h-7 max-w-[200px] cursor-pointer items-center gap-1 rounded-t-md border border-b-0 px-2 text-[12px] transition-colors ${
                        isActive
                          ? 'border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-primary)]'
                          : 'border-transparent bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                      }`}
                    >
                      <span
                        className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]"
                        aria-hidden="true"
                      >
                        public
                      </span>
                      <span className="truncate">{label}</span>
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

            <div className="flex items-center gap-1 border-b border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] px-2 py-1.5">
              <NavBtn icon="arrow_back" title={t('browser.panel.back')} onClick={() => sessionId && void back(sessionId)} />
              <NavBtn icon="arrow_forward" title={t('browser.panel.forward')} onClick={() => sessionId && void forward(sessionId)} />
              <NavBtn
                icon="refresh"
                title={t('browser.panel.reload')}
                onClick={() => sessionId && void reload(sessionId, false)}
              />
              <form onSubmit={onSubmitUrl} className="flex flex-1 items-center">
                <input
                  type="text"
                  value={draftUrl}
                  onChange={(e) => setDraftUrl(e.target.value)}
                  placeholder={t('browser.panel.urlPlaceholder')}
                  className="h-7 w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
                  spellCheck={false}
                  autoCapitalize="off"
                  autoCorrect="off"
                />
              </form>
              <div className="ml-1 flex items-center gap-1">
                <ToolbarToggleBtn
                  icon="ads_click"
                  title={t('browser.panel.pickElement')}
                  active={pickMode}
                  onClick={() => sessionId && void togglePick(sessionId)}
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
            <div
              ref={viewportRef}
              data-testid="embedded-browser-viewport"
              className="relative w-full bg-[var(--color-surface)]"
              style={{ height: PANEL_HEIGHT_PX }}
            >
              {!ownsDock && (
                <div className="absolute inset-0 flex items-center justify-center text-[12px] text-[var(--color-text-tertiary)]">
                  {t('browser.panel.empty')}
                </div>
              )}
            </div>
            {consoleOpen && (
              <ConsoleDrawer
                entries={consoleLog}
                onClear={() => sessionId && clearConsole(sessionId)}
                title={t('browser.panel.consoleTitle')}
                emptyLabel={t('browser.panel.consoleEmpty')}
                clearLabel={t('browser.panel.consoleClear')}
              />
            )}
            {inspectorOpen && (
              <InspectorDrawer
                snapshot={inspector}
                emptyLabel={t('browser.panel.inspectorEmpty')}
                title={t('browser.panel.inspectorTitle')}
              />
            )}
            {driverOpen && (
              <DriverDrawer
                entries={agentLog}
                onClear={() => sessionId && clearAgentLog(sessionId)}
                title={t('browser.panel.driver.title')}
                emptyLabel={t('browser.panel.driver.empty')}
                clearLabel={t('browser.panel.driver.clear')}
              />
            )}
          </>
        )}
        </div>
      </div>
    </div>
  )
}

function NavBtn({ icon, title, onClick }: { icon: string; title: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      className="inline-flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
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
}: {
  icon: string
  title: string
  active?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      aria-pressed={active}
      className={`inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors ${
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
    <div
      className="flex flex-col border-t border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]"
      style={{ height: CONSOLE_DRAWER_HEIGHT_PX }}
    >
      <div className="flex items-center justify-between px-3 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]">
        <span>{title}</span>
        <button
          type="button"
          onClick={onClear}
          className="rounded px-2 py-0.5 text-[11px] hover:bg-[var(--color-surface-hover)]"
        >
          {clearLabel}
        </button>
      </div>
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-3 pb-1 font-mono text-[11px]">
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
    <div
      className="flex flex-col border-t border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]"
      style={{ height: INSPECTOR_DRAWER_HEIGHT_PX }}
    >
      <div className="flex items-center justify-between px-3 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]">
        <span>{title}</span>
        {snapshot ? (
          <span className="truncate text-[10px] text-[var(--color-text-tertiary)]">{snapshot.selector}</span>
        ) : null}
      </div>
      <div className="flex-1 overflow-y-auto px-3 pb-1 font-mono text-[11px]">
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

function DriverDrawer({
  entries,
  onClear,
  title,
  emptyLabel,
  clearLabel,
}: {
  entries: BrowserAgentActionEntry[]
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

  const formatArgs = (raw: unknown): string => {
    if (raw == null) return ''
    if (typeof raw === 'string') return raw.length > 160 ? `${raw.slice(0, 160)}…` : raw
    try {
      const text = JSON.stringify(raw)
      return text.length > 160 ? `${text.slice(0, 160)}…` : text
    } catch {
      return String(raw)
    }
  }

  return (
    <div
      className="flex flex-col border-t border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]"
      style={{ height: DRIVER_DRAWER_HEIGHT_PX }}
    >
      <div className="flex items-center justify-between px-3 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]">
        <span>{title}</span>
        <button
          type="button"
          onClick={onClear}
          className="rounded px-2 py-0.5 text-[11px] hover:bg-[var(--color-surface-hover)]"
        >
          {clearLabel}
        </button>
      </div>
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-3 pb-1 font-mono text-[11px]">
        {entries.length === 0 ? (
          <div className="py-2 text-[var(--color-text-tertiary)]">{emptyLabel}</div>
        ) : (
          entries.map((entry) => (
            <div key={entry.id} className="flex items-baseline gap-2 py-0.5">
              <span className="opacity-60">{new Date(entry.ts).toLocaleTimeString()}</span>
              <span className="rounded bg-[var(--color-brand)]/10 px-1.5 text-[10px] font-semibold uppercase text-[var(--color-brand)]">
                {entry.kind}
              </span>
              <span className="break-all text-[var(--color-text-primary)]">{formatArgs(entry.args)}</span>
            </div>
          ))
        )}
      </div>
    </div>
  )
}

export const BROWSER_PANEL_HEIGHTS = {
  expanded: PANEL_HEIGHT_PX,
  collapsed: PANEL_HEIGHT_COLLAPSED_PX,
} as const
