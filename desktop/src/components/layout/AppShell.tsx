// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { Sidebar } from './Sidebar'
import { ContentRouter } from './ContentRouter'
import { ToastContainer } from '../shared/Toast'
import { UpdateChecker } from '../shared/UpdateChecker'
import { CodingModeTransitionGuard } from '../controls/CodingModeTransitionGuard'
import { QuickModeSwitcher } from '../controls/QuickModeSwitcher'
import { useSettingsStore } from '../../stores/settingsStore'
import { RIGHT_SIDEBAR_BOUNDS, useUIStore } from '../../stores/uiStore'
import { useKeyboardShortcuts } from '../../hooks/useKeyboardShortcuts'
import { useTerminalCwdSync } from '../../hooks/useTerminalCwdSync'
import {
  DESKTOP_RUNTIME_TUNABLES,
  fetchSettingsWithRetry,
  getServerStatusSnapshot,
  initializeDesktopServerUrl,
  openLogDir,
  requestGatewayRestart,
  subscribeServerStatus,
  type DesktopBootEvent,
  type ServerStatusSnapshot,
} from '../../lib/desktopRuntime'
import { startAiWriteWatcher } from '../../lib/aiWriteWatcher'
import { TabBar } from './TabBar'
import { TitleBar } from './TitleBar'
import { ResizeHandleRight } from './ResizeHandleRight'
import { ResizeHandleBrowser } from './ResizeHandleBrowser'
import { ResizeHandles } from './ResizeHandles'
import { StatusBar } from './StatusBar'
import { useTabStore } from '../../stores/tabStore'
import { focusSession } from '../../lib/focusSession'
import { useSessionRunStateStore } from '../../stores/sessionRunStateStore'
import { useTranslation } from '../../i18n'
import { RightSidebar } from '../workspace/RightSidebar'
import { WorkspaceFinder } from '../workspace/WorkspaceFinder'
import { useActiveTabWorkDir } from '../../lib/activeWorkDir'
import { Settings } from '../../pages/Settings'
import { EmbeddedBrowserPanel } from '../chat/EmbeddedBrowserPanel'
import { TerminalPanel } from '../terminal/TerminalPanel'
import { startBackgroundShellMirror } from '../../api/backgroundShell'
import { useTerminalPanelStore } from '../../stores/terminalPanelStore'
import { useBrowserPanelStore } from '../../stores/browserPanelStore'
import { dockHide, dockSetForegroundSession } from '../../lib/browserDock'
import { isTauriRuntime } from '../../lib/desktopRuntime'

export function AppShell() {
  const fetchSettings = useSettingsStore((s) => s.fetchAll)
  const sidebarOpen = useUIStore((s) => s.sidebarOpen)
  const rightSidebarOpen = useUIStore((s) => s.rightSidebarOpen)
  const workspaceFinderMode = useUIStore((s) => s.workspaceFinderMode)
  const closeWorkspaceFinder = useUIStore((s) => s.closeWorkspaceFinder)
  const activeWorkDir = useActiveTabWorkDir()
  const settingsOverlayOpen = useUIStore((s) => s.settingsOverlayOpen)
  const terminalPanelOpen = useTerminalPanelStore((s) => s.open)
  const activeChatTabId = useTabStore((s) => s.activeTabId)
  const browserPanelVisible = useBrowserPanelStore((s) =>
    activeChatTabId ? s.panels[activeChatTabId]?.visible ?? false : false,
  )
  const [ready, setReady] = useState(false)
  const [settingsMounted, setSettingsMounted] = useState(false)
  const [bootElapsedSecs, setBootElapsedSecs] = useState(0)
  const [bootLastEvent, setBootLastEvent] = useState<DesktopBootEvent | null>(null)
  const [bootStatus, setBootStatus] = useState<ServerStatusSnapshot | null>(null)
  const [bootFailed, setBootFailed] = useState(false)
  const [retrying, setRetrying] = useState(false)
  const [copiedError, setCopiedError] = useState(false)

  const [isMaximized, setIsMaximized] = useState(false)
  const t = useTranslation()

  useEffect(() => {
    if (ready) return
    const startedAt = Date.now()
    const interval = window.setInterval(() => {
      setBootElapsedSecs(Math.floor((Date.now() - startedAt) / 1000))
    }, 1_000)
    const statusPoll = window.setInterval(() => {
      void (async () => {
        const snap = await getServerStatusSnapshot()
        if (snap) setBootStatus(snap)
      })()
    }, DESKTOP_RUNTIME_TUNABLES.STATUS_FALLBACK_POLL_MS)
    let unlistenStatus: (() => void) | null = null
    void (async () => {
      const snap = await getServerStatusSnapshot()
      if (snap) setBootStatus(snap)
      unlistenStatus = await subscribeServerStatus((payload) => {
        setBootStatus(payload)
      })
    })()
    return () => {
      window.clearInterval(interval)
      window.clearInterval(statusPoll)
      if (unlistenStatus) unlistenStatus()
    }
  }, [ready])

  useEffect(() => {
    if (ready) return
    if (!bootStatus) return
    if (bootStatus.state === 'failed') {
      setBootFailed(true)
    }
  }, [bootStatus, ready])

  useEffect(() => {
    if (settingsOverlayOpen) setSettingsMounted(true)
  }, [settingsOverlayOpen])

  useEffect(() => {
    const abort = new AbortController()

    const bootstrap = async () => {
      try {
        await initializeDesktopServerUrl({
          signal: abort.signal,
          onEvent: (event) => {
            if (abort.signal.aborted) return
            setBootLastEvent(event)
            if (event.kind === 'bootstrap-failed') {
              setBootFailed(true)
            }
          },
        })
        await fetchSettingsWithRetry(fetchSettings, { signal: abort.signal })
        startBackgroundShellMirror()
        useSessionRunStateStore.getState().start()

        const RESTORE_MAX_ATTEMPTS = 6
        let restoreAttempts = 0
        while (!abort.signal.aborted && restoreAttempts < RESTORE_MAX_ATTEMPTS) {
          try {
            await useTabStore.getState().restoreTabs()
            break
          } catch (err) {
            restoreAttempts += 1
            if (restoreAttempts >= RESTORE_MAX_ATTEMPTS) {
              console.warn(
                '[desktop] restoreTabs still failing after retries; rendering main UI without restored tabs so the user can recreate them',
                err,
              )
              break
            }
            await new Promise<void>((resolve) => setTimeout(resolve, 400))
          }
        }

        if (abort.signal.aborted) {
          return
        }

        const { activeTabId: activeId, tabs } = useTabStore.getState()
        const activeTab = tabs.find((tab) => tab.sessionId === activeId)
        if (activeId && activeTab?.type === 'session') {
          focusSession(activeId)
        }
        if (abort.signal.aborted) {
          return
        }
        setReady(true)
      } catch (error) {
        if (!abort.signal.aborted) {
          setBootFailed(true)
          console.error('[desktop] bootstrap aborted with unrecoverable error', error)
        }
      }
    }

    void bootstrap()

    return () => abort.abort()
  }, [fetchSettings])

  useEffect(() => {
    let cancelled = false
    const reveal = async () => {
      try {
        const [{ invoke }, { getCurrentWindow }] = await Promise.all([
          import(/* @vite-ignore */ '@tauri-apps/api/core'),
          import(/* @vite-ignore */ '@tauri-apps/api/window'),
        ])
        if (cancelled) return
        try {
          await invoke('signal_frontend_ready')
        } catch {
          const win = getCurrentWindow()
          try {
            await win.show()
          } catch {

          }
          try {
            await win.setFocus()
          } catch {

          }
        }
      } catch {

      }
    }
    const raf = window.requestAnimationFrame(() => {
      void reveal()
    })
    return () => {
      cancelled = true
      window.cancelAnimationFrame(raf)
    }
  }, [])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    import(/* @vite-ignore */ '@tauri-apps/api/event')
      .then(({ listen }) =>
        listen<string>('native-menu-navigate', () => {
          useUIStore.getState().toggleSettingsOverlay()
        }),
      )
      .then((fn) => { unlisten = fn })
      .catch(() => {})
    return () => { unlisten?.() }
  }, [])

  useEffect(() => {
    const dispose = startAiWriteWatcher()
    return () => dispose()
  }, [])

  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined
    void import(/* @vite-ignore */ '@tauri-apps/api/window')
      .then(async ({ getCurrentWindow }) => {
        if (cancelled) return
        const win = getCurrentWindow()
        const sync = async () => {
          if (cancelled) return
          try {
            setIsMaximized(await win.isMaximized())
          } catch {

          }
        }
        await sync()
        const fn = await win.onResized(() => {
          void sync()
        })
        if (cancelled) {
          fn()
        } else {
          unlisten = fn
        }
      })
      .catch(() => {

      })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useKeyboardShortcuts()
  useTerminalCwdSync()

  useEffect(() => {
    const root = document.documentElement
    if (!root.style.getPropertyValue('--composer-height')) {
      root.style.setProperty('--composer-height', '0px')
    }
  }, [])

  useEffect(() => {
    const dispatchRemeasure = () => {
      document.dispatchEvent(new CustomEvent('browser-panel-remeasure'))
    }
    const dispatchResync = () => {
      document.dispatchEvent(new CustomEvent('browser-panel-resync'))
    }
    dispatchRemeasure()
    const resyncTimers = [120, 320, 600, 950].map((ms) =>
      window.setTimeout(() => {
        dispatchRemeasure()
        dispatchResync()
      }, ms),
    )
    return () => {
      for (const id of resyncTimers) window.clearTimeout(id)
    }
  }, [isMaximized])

  useEffect(() => {
    if (!isTauriRuntime()) return
    dockSetForegroundSession(activeChatTabId ?? null).catch((err) => {
      console.warn('[browserDock] set foreground session failed', err)
    })
    if (!activeChatTabId) {
      dockHide().catch((err) => {
        console.warn('[browserDock] dockHide on empty tab change failed', err)
      })
      return
    }
    const previousTabId = activeChatTabId
    return () => {
      useBrowserPanelStore.setState((state) => (
        state.activeSessionId === previousTabId
          ? { activeSessionId: null }
          : state
      ))
      dockHide().catch((err) => {
        console.warn('[browserDock] dockHide on active tab change failed', err)
      })
    }
  }, [activeChatTabId])

  if (!ready) {
    const showHint = bootElapsedSecs >= 6
    const showActions = bootFailed || bootElapsedSecs >= 10
    const showLongHint = bootElapsedSecs >= 20 && !bootFailed
    const ERROR_EVENT_KINDS = new Set(['health-failed', 'bootstrap-failed'])
    const isLikelyUrl = (value: string) =>
      /^(https?|wss?|tauri):\/\//i.test(value) || /^127\.0\.0\.1[:/]/.test(value)
    const rawEventDetail = bootLastEvent?.detail?.trim()
    const lastEventDetail =
      rawEventDetail &&
      bootLastEvent &&
      ERROR_EVENT_KINDS.has(bootLastEvent.kind) &&
      !isLikelyUrl(rawEventDetail)
        ? rawEventDetail
        : null
    const rawStatusError = bootStatus?.error?.trim()
    const statusError =
      rawStatusError && !isLikelyUrl(rawStatusError) ? rawStatusError : null
    const surfacedError = statusError || lastEventDetail || null
    const handleRetry = async () => {
      if (retrying) return
      setRetrying(true)
      try {
        const ok = await requestGatewayRestart(true)
        if (ok) {
          setBootFailed(false)
        }
      } finally {
        setRetrying(false)
      }
    }
    const handleOpenLogDir = () => {
      void openLogDir()
    }
    const handleCopyError = async () => {
      const text = surfacedError ?? ''
      if (!text) return
      try {
        if (navigator.clipboard?.writeText) {
          await navigator.clipboard.writeText(text)
        } else {
          const ta = document.createElement('textarea')
          ta.value = text
          ta.style.position = 'fixed'
          ta.style.opacity = '0'
          document.body.appendChild(ta)
          ta.select()
          document.execCommand('copy')
          document.body.removeChild(ta)
        }
        setCopiedError(true)
        window.setTimeout(() => setCopiedError(false), 2_000)
      } catch (err) {
        console.warn('[desktop] copy launch error failed', err)
      }
    }
    const headerText = bootFailed
      ? t('app.launchingFailed')
      : t('app.launching')
    const slowText = bootFailed
      ? t('app.launchingFailedDetail').replace(
          '{{seconds}}',
          String(bootElapsedSecs),
        )
      : t('app.launchingSlow').replace(
          '{{seconds}}',
          String(bootElapsedSecs),
        )
    return (
      <>
        <div
          className="app-window-backdrop"
          data-maximized={isMaximized ? 'true' : 'false'}
        />
        <div
          className="app-window-frame items-center justify-center text-[var(--color-text-secondary)]"
          data-maximized={isMaximized ? 'true' : 'false'}
        >
          <div className="flex flex-col items-center gap-3 text-center px-6 max-w-[560px]">
            <div data-state={bootFailed ? 'failed' : 'starting'}>{headerText}</div>
            {(showHint || bootFailed) && (
              <div className="text-xs text-[var(--color-text-tertiary)]">
                {slowText}
              </div>
            )}
            {showLongHint && (
              <div className="text-xs text-[var(--color-text-tertiary)] opacity-80">
                {t('app.launchingTip')}
              </div>
            )}
            {surfacedError && (
              <div className="max-h-[120px] w-full overflow-auto rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-left text-[11px] text-[var(--color-text-tertiary)]">
                {surfacedError}
              </div>
            )}
            {showActions && (
              <div className="mt-1 flex flex-wrap items-center justify-center gap-2">
                <button
                  type="button"
                  onClick={() => void handleRetry()}
                  disabled={retrying}
                  className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-1 text-xs text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-60"
                >
                  {retrying ? t('app.retrying') : t('app.retry')}
                </button>
                <button
                  type="button"
                  onClick={handleOpenLogDir}
                  className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-1 text-xs text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
                >
                  {t('app.openLogDir')}
                </button>
                {surfacedError && (
                  <button
                    type="button"
                    onClick={() => void handleCopyError()}
                    className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-1 text-xs text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
                  >
                    {copiedError
                      ? t('app.copyLaunchErrorCopied')
                      : t('app.copyLaunchError')}
                  </button>
                )}
              </div>
            )}
          </div>
        </div>
        <ResizeHandles disabled={isMaximized} />
      </>
    )
  }

  return (

    <>
      <div
        className="app-window-backdrop"
        data-maximized={isMaximized ? 'true' : 'false'}
      />
      <div
        className="app-window-frame outline-none ring-0"
        data-maximized={isMaximized ? 'true' : 'false'}
      >
      <TitleBar />
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <div
          data-testid="sidebar-shell"
          data-state={sidebarOpen ? 'open' : 'closed'}
          className="sidebar-shell"
        >
          <Sidebar />
        </div>
        <div className="relative flex-1 flex min-w-0 overflow-hidden">
          <div
            className="relative flex flex-1 flex-col overflow-hidden"
            style={{ minWidth: RIGHT_SIDEBAR_BOUNDS.mainAreaMin }}
          >
            <main
              id="content-area"
              data-sidebar-state={sidebarOpen ? 'open' : 'closed'}
              className="min-w-0 flex-1 flex flex-col overflow-hidden"
            >
              <TabBar />
              <ContentRouter />
            </main>
            {settingsMounted && (
              <div
                aria-hidden={!settingsOverlayOpen}
                className={
                  settingsOverlayOpen
                    ? 'absolute inset-0 z-30 flex flex-col bg-[var(--color-surface)]'
                    : 'hidden'
                }
              >
                <Settings />
              </div>
            )}
          </div>
          {browserPanelVisible && (
            <>
              <ResizeHandleBrowser />
              <EmbeddedBrowserPanel />
            </>
          )}
          <div
            className={rightSidebarOpen ? 'contents' : 'hidden'}
          >
            <ResizeHandleRight />
            <RightSidebar />
          </div>
        </div>
      </div>
      {terminalPanelOpen && <TerminalPanel />}
      <StatusBar />
      <ToastContainer />
      <UpdateChecker />
      <CodingModeTransitionGuard />
      <QuickModeSwitcher />
      {workspaceFinderMode && (
        <WorkspaceFinder
          mode={workspaceFinderMode}
          workDir={activeWorkDir}
          onClose={closeWorkspaceFinder}
        />
      )}
      </div>
      <ResizeHandles disabled={isMaximized} />
    </>
  )
}
