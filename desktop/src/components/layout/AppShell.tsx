import { useEffect, useState } from 'react'
import { Sidebar } from './Sidebar'
import { ContentRouter } from './ContentRouter'
import { ToastContainer } from '../shared/Toast'
import { UpdateChecker } from '../shared/UpdateChecker'
import { CodingModeTransitionGuard } from '../controls/CodingModeTransitionGuard'
import { QuickModeSwitcher } from '../controls/QuickModeSwitcher'
import { useSettingsStore } from '../../stores/settingsStore'
import { useUIStore } from '../../stores/uiStore'
import { useKeyboardShortcuts } from '../../hooks/useKeyboardShortcuts'
import { useTerminalCwdSync } from '../../hooks/useTerminalCwdSync'
import {
  fetchSettingsWithRetry,
  initializeDesktopServerUrl,
} from '../../lib/desktopRuntime'
import { startAiWriteWatcher } from '../../lib/aiWriteWatcher'
import { TabBar } from './TabBar'
import { TitleBar } from './TitleBar'
import { ResizeHandleRight } from './ResizeHandleRight'
import { ResizeHandles } from './ResizeHandles'
import { StatusBar } from './StatusBar'
import { useTabStore } from '../../stores/tabStore'
import { useChatStore } from '../../stores/chatStore'
import { useTranslation } from '../../i18n'
import { RightSidebar } from '../workspace/RightSidebar'
import { Settings } from '../../pages/Settings'
import { TerminalPanel } from '../terminal/TerminalPanel'
import { startBackgroundShellMirror } from '../../api/backgroundShell'
import { useTerminalPanelStore } from '../../stores/terminalPanelStore'

export function AppShell() {
  const fetchSettings = useSettingsStore((s) => s.fetchAll)
  const sidebarOpen = useUIStore((s) => s.sidebarOpen)
  const rightSidebarOpen = useUIStore((s) => s.rightSidebarOpen)
  const settingsOverlayOpen = useUIStore((s) => s.settingsOverlayOpen)
  const terminalPanelOpen = useTerminalPanelStore((s) => s.open)
  const [ready, setReady] = useState(false)
  const [settingsMounted, setSettingsMounted] = useState(false)

  const [isMaximized, setIsMaximized] = useState(false)
  const t = useTranslation()

  useEffect(() => {
    if (settingsOverlayOpen) setSettingsMounted(true)
  }, [settingsOverlayOpen])

  useEffect(() => {
    const abort = new AbortController()

    const bootstrap = async () => {
      try {
        await initializeDesktopServerUrl({ signal: abort.signal })
        await fetchSettingsWithRetry(fetchSettings, { signal: abort.signal })
        startBackgroundShellMirror()

        while (!abort.signal.aborted) {
          try {
            await useTabStore.getState().restoreTabs()
            break
          } catch {
            await new Promise<void>((resolve) => setTimeout(resolve, 400))
          }
        }

        if (abort.signal.aborted) {
          return
        }

        const { activeTabId: activeId, tabs } = useTabStore.getState()
        const activeTab = tabs.find((tab) => tab.sessionId === activeId)
        if (activeId && activeTab?.type === 'session') {
          useChatStore.getState().connectToSession(activeId)
        }
        if (abort.signal.aborted) {
          return
        }
        setReady(true)
      } catch {

      }
    }

    void bootstrap()

    return () => abort.abort()
  }, [fetchSettings])

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

  if (!ready) {
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
          {t('app.launching')}
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
          <main
            id="content-area"
            data-sidebar-state={sidebarOpen ? 'open' : 'closed'}
            className="min-w-0 flex-1 flex flex-col overflow-hidden"
          >
            <TabBar />
            <ContentRouter />
          </main>
          <div
            className={rightSidebarOpen ? 'contents' : 'hidden'}
          >
            <ResizeHandleRight />
            <RightSidebar />
          </div>
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
      </div>
      {terminalPanelOpen && <TerminalPanel />}
      <StatusBar />
      <ToastContainer />
      <UpdateChecker />
      <CodingModeTransitionGuard />
      <QuickModeSwitcher />
      </div>
      <ResizeHandles disabled={isMaximized} />
    </>
  )
}
