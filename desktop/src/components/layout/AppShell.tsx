// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { lazy, Suspense, useEffect, useState } from 'react'
import { Sidebar } from './Sidebar'
import { ContentRouter } from './ContentRouter'
import { ToastContainer } from '../shared/Toast'
import { UpdateChecker } from '../shared/UpdateChecker'
import { CodingModeTransitionGuard } from '../controls/CodingModeTransitionGuard'
import { QuickModeSwitcher } from '../controls/QuickModeSwitcher'
import { CommandPalette } from '../controls/CommandPalette'
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
import { markWindowBusy } from '../../lib/windowBusy'
import { startTaskbarAlertWatcher } from '../../lib/taskbarAlert'
import { TabBar } from './TabBar'
import { TitleBar } from './TitleBar'
import { BuddyCompanion } from './BuddyCompanion'
import { ResizeHandleRight } from './ResizeHandleRight'
import { ResizeHandleBrowser } from './ResizeHandleBrowser'
import { ResizeHandles } from './ResizeHandles'
import { StatusBar } from './StatusBar'
import { useTabStore, SCHEDULED_TAB_ID } from '../../stores/tabStore'
import { focusSession } from '../../lib/focusSession'
import { useSessionRunStateStore } from '../../stores/sessionRunStateStore'
import { useLspStore } from '../../stores/lspStore'
import { translate, useTranslation } from '../../i18n'
import { RightSidebar } from '../workspace/RightSidebar'
import { WorkspaceFinder } from '../workspace/WorkspaceFinder'
import { FileDragGhost } from '../workspace/FileDragGhost'
import { useActiveTabWorkDir } from '../../lib/activeWorkDir'
const Settings = lazy(() =>
  import('../../pages/Settings').then((m) => ({ default: m.Settings })),
)
const TemplateLibrary = lazy(() =>
  import('../../pages/TemplateLibrary').then((m) => ({ default: m.TemplateLibrary })),
)
const SharePanel = lazy(() =>
  import('../lanShare/SharePanel').then((m) => ({ default: m.SharePanel })),
)
const ReviewPanel = lazy(() =>
  import('../chat/ReviewPanel').then((m) => ({ default: m.ReviewPanel })),
)
import { EmbeddedBrowserPanel } from '../chat/EmbeddedBrowserPanel'
const DesignerCanvasPanel = lazy(() =>
  import('../designer/DesignerCanvasPanel').then((m) => ({
    default: m.DesignerCanvasPanel,
  })),
)
import { ResizeHandleCanvas } from './ResizeHandleCanvas'
import { useDesignerCanvasStore } from '../../stores/designerCanvasStore'
import { DesignerDockCoordinator } from './DesignerDockCoordinator'
import { TerminalPanel } from '../terminal/TerminalPanel'
import { startBackgroundShellMirror } from '../../api/backgroundShell'
import { useBrowserPanelStore } from '../../stores/browserPanelStore'
import { dockHide, dockSetForegroundSession } from '../../lib/browserDock'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { getBaseUrl, setBaseUrl } from '../../api/client'
import { wsManager } from '../../api/websocket'
import { handleCloseRequest, performSafeExit } from '../../lib/appClose'
import {
  MINIMAL_EVENT_ACTIVE_SESSION,
  MINIMAL_EVENT_COMPUTER_EXIT,
  MINIMAL_EVENT_COMPUTER_PROGRESS,
  MINIMAL_EVENT_COMPUTER_REPLAY,
  MINIMAL_EVENT_COMPUTER_REPLY,
  MINIMAL_EVENT_COMPUTER_START,
  MINIMAL_EVENT_COMPUTER_STEER,
  MINIMAL_EVENT_COMPUTER_STOP,
  MINIMAL_EVENT_COMPUTER_SYNC,
  MINIMAL_EVENT_RECORDER_CONTROL,
  MINIMAL_EVENT_RECORDER_PROGRESS,
  MINIMAL_EVENT_RECORDER_SYNC,
  MINIMAL_EVENT_OPEN_SETTINGS,
  MINIMAL_EVENT_STOP,
  MINIMAL_EVENT_SUBMIT,
} from '../../lib/minimalMode'
import type {
  MinimalComputerProgress,
  MinimalComputerReplay,
  MinimalComputerReply,
  MinimalComputerStart,
  MinimalComputerSteer,
  MinimalRecorderControl,
  MinimalRecorderProgress,
  MinimalSubmitPayload,
} from '../../lib/minimalMode'
import { useChatStore } from '../../stores/chatStore'
import { useSessionRuntimeStore } from '../../stores/sessionRuntimeStore'
import { useComputerUseStore } from '../../stores/computerUseStore'
import { useComputerRecorderStore } from '../../stores/computerRecorderStore'
import { useLanShareStore } from '../../stores/lanShareStore'
import { useReviewPanelStore } from '../../stores/reviewPanelStore'
import { CloseChoiceModal } from './CloseChoiceModal'
import { SafeExitOverlay } from './SafeExitOverlay'
import { ComputerUsePage } from '../../pages/ComputerUse'

export function AppShell() {
  const fetchSettings = useSettingsStore((s) => s.fetchAll)
  const sidebarOpen = useUIStore((s) => s.sidebarOpen)
  const rightSidebarOpen = useUIStore((s) => s.rightSidebarOpen)
  const workspaceFinderMode = useUIStore((s) => s.workspaceFinderMode)
  const closeWorkspaceFinder = useUIStore((s) => s.closeWorkspaceFinder)
  const activeWorkDir = useActiveTabWorkDir()
  const settingsOverlayOpen = useUIStore((s) => s.settingsOverlayOpen)
  const templateLibraryOpen = useUIStore((s) => s.templateLibraryOpen)
  const lanSharePanelOpen = useLanShareStore((s) => s.panelOpen)
  const reviewPanelOpen = useReviewPanelStore((s) => s.open)
  const appMode = useUIStore((s) => s.appMode)
  const activeChatTabId = useTabStore((s) => s.activeTabId)
  const activeChatTitle = useTabStore((s) =>
    s.activeTabId ? s.tabs.find((tab) => tab.sessionId === s.activeTabId)?.title ?? null : null,
  )
  const activeChatTabType = useTabStore((s) =>
    s.activeTabId ? s.tabs.find((tab) => tab.sessionId === s.activeTabId)?.type ?? null : null,
  )
  const browserPanelVisible = useBrowserPanelStore((s) =>
    activeChatTabId ? s.panels[activeChatTabId]?.visible ?? false : false,
  )
  const designerCanvasVisible = useDesignerCanvasStore((s) =>
    activeChatTabId ? s.panels[activeChatTabId]?.visible ?? false : false,
  )
  const [ready, setReady] = useState(false)
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
    let disposed = false
    let unlistenStatus: (() => void) | null = null
    void (async () => {
      const snap = await getServerStatusSnapshot()
      if (disposed) return
      if (snap) setBootStatus(snap)
      const unlisten = await subscribeServerStatus((payload) => {
        setBootStatus(payload)
      })
      if (disposed) {
        unlisten()
        return
      }
      unlistenStatus = unlisten
    })()
    return () => {
      disposed = true
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
    let lastShownAt = 0
    const handler = (event: Event) => {
      const detail = (event as CustomEvent).detail as
        | { label?: string; message?: string }
        | undefined
      const message = detail?.message?.trim()
      if (!message) return
      const now = Date.now()
      if (now - lastShownAt < 3000) return
      lastShownAt = now
      useUIStore.getState().addToast({
        type: 'error',
        message: t('app.runtimeError', { message }),
        duration: 8000,
      })
    }
    window.addEventListener('app:runtime-error', handler as EventListener)
    return () => {
      window.removeEventListener('app:runtime-error', handler as EventListener)
    }
  }, [t])

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
        void useLspStore.getState().fetch().catch(() => {})

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
        void import('../../stores/settingsStore').then(({ syncLocaleToShell, useSettingsStore }) =>
          syncLocaleToShell(useSettingsStore.getState().locale),
        )
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
    const timer = window.setTimeout(() => {
      void reveal()
    }, 0)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [])

  useEffect(() => {
    if (!ready) return
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      const off = await subscribeServerStatus((snap) => {
        if (snap.state !== 'ready') return
        const url = snap.url?.trim()
        if (!url) return
        const normalized = url.replace(/\/$/, '')
        if (normalized === getBaseUrl().replace(/\/$/, '')) return
        setBaseUrl(normalized)
        useSessionRunStateStore.getState().stop()
        useSessionRunStateStore.getState().start()
        wsManager.forceReconnectAll()
      })
      if (disposed) {
        off()
      } else {
        unlisten = off
      }
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [ready])

  useEffect(() => {
    const dispose = startAiWriteWatcher()
    return () => dispose()
  }, [])

  useEffect(() => {
    const dispose = startTaskbarAlertWatcher()
    return () => dispose()
  }, [])

  useEffect(() => {
    let cancelled = false
    let unlistenResized: (() => void) | undefined
    let unlistenMoved: (() => void) | undefined
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
        const fnResized = await win.onResized(() => {
          markWindowBusy()
          void sync()
        })
        const fnMoved = await win.onMoved(() => {
          markWindowBusy()
        })
        if (cancelled) {
          fnResized()
          fnMoved()
        } else {
          unlistenResized = fnResized
          unlistenMoved = fnMoved
        }
      })
      .catch(() => {

      })
    return () => {
      cancelled = true
      unlistenResized?.()
      unlistenMoved?.()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let cancelled = false
    let unlistenClose: (() => void) | undefined
    let unlistenTrayQuit: (() => void) | undefined
    void (async () => {
      try {
        const [{ getCurrentWindow }, { listen }] = await Promise.all([
          import('@tauri-apps/api/window'),
          import('@tauri-apps/api/event'),
        ])
        if (cancelled) return
        const offClose = await getCurrentWindow().onCloseRequested((event) => {
          event.preventDefault()
          void handleCloseRequest()
        })
        const offTrayQuit = await listen('tray://quit-requested', () => {
          void performSafeExit()
        })
        if (cancelled) {
          offClose()
          offTrayQuit()
        } else {
          unlistenClose = offClose
          unlistenTrayQuit = offTrayQuit
        }
      } catch (err) {
        console.warn('[appClose] failed to register close handlers', err)
      }
    })()
    return () => {
      cancelled = true
      unlistenClose?.()
      unlistenTrayQuit?.()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event')
      const off = await listen(MINIMAL_EVENT_OPEN_SETTINGS, () => {
        useUIStore.getState().openSettingsOverlay()
      })
      if (disposed) off()
      else unlisten = off
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    const forwardable =
      !!activeChatTabId &&
      activeChatTabId !== SCHEDULED_TAB_ID &&
      (activeChatTabType === null || activeChatTabType === 'session')
    void (async () => {
      try {
        const { emit } = await import('@tauri-apps/api/event')
        await emit(
          MINIMAL_EVENT_ACTIVE_SESSION,
          forwardable ? { id: activeChatTabId, title: activeChatTitle } : null,
        )
      } catch {

      }
    })()
  }, [activeChatTabId, activeChatTitle, activeChatTabType])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    const offs: Array<() => void> = []
    const register = (off: () => void) => {
      if (disposed) off()
      else offs.push(off)
    }
    void (async () => {
      const { listen, emit } = await import('@tauri-apps/api/event')
      register(
        await listen<MinimalSubmitPayload>(MINIMAL_EVENT_SUBMIT, (event) => {
          const payload = event.payload
          if (!payload?.sessionId || typeof payload.content !== 'string') return
          useSessionRuntimeStore.getState().reloadFromStorage()
          useChatStore
            .getState()
            .sendMessage(payload.sessionId, payload.content, payload.attachments, payload.options)
        }),
      )
      register(
        await listen<string>(MINIMAL_EVENT_STOP, (event) => {
          const sessionId = event.payload
          if (!sessionId) return
          useChatStore.getState().stopGeneration(sessionId)
        }),
      )

      let lastProgressKey = ''
      const emitComputerProgress = async (force = false) => {
        const s = useComputerUseStore.getState()
        const last = s.steps.length > 0 ? s.steps[s.steps.length - 1] : null
        const lastUserUpdate = (() => {
          for (let i = s.steps.length - 1; i >= 0; i--) {
            const step = s.steps[i]
            if (step && step.kind === 'user_update') return step.thought
          }
          return null
        })()
        const payload: MinimalComputerProgress = {
          status: s.status,
          statusMessage: s.statusMessage,
          error: s.error,
          lastThought: last?.thought ? last.thought : null,
          lastAction: last ? last.elementDescription || last.actionType || null : null,
          stepCount: s.steps.filter((step) => step.kind === 'action').length,
          pendingSteer: s.pendingSteer,
          lastUserUpdate,
        }
        const key = JSON.stringify(payload)
        if (!force && key === lastProgressKey) return
        lastProgressKey = key
        try {
          await emit(MINIMAL_EVENT_COMPUTER_PROGRESS, payload)
        } catch {

        }
      }
      register(
        await listen<MinimalComputerStart>(MINIMAL_EVENT_COMPUTER_START, (event) => {
          const p = event.payload
          if (!p?.task?.trim()) return
          const rec = useComputerRecorderStore.getState()
          if (rec.status === 'recording') return
          const cu = useComputerUseStore.getState()
          if (p.provider && p.model) cu.setSelection(p.provider, p.model)
          cu.setTask(p.task)
          cu.start(
            p.attachments && p.attachments.length > 0
              ? { attachments: p.attachments }
              : undefined,
          )
        }),
      )
      register(
        await listen(MINIMAL_EVENT_COMPUTER_STOP, () => {
          useComputerUseStore.getState().stop()
        }),
      )
      register(
        await listen<MinimalComputerReply>(MINIMAL_EVENT_COMPUTER_REPLY, (event) => {
          const text = event.payload?.text
          if (!text?.trim()) return
          useComputerUseStore.getState().sendReply(text)
        }),
      )
      register(
        await listen<MinimalComputerSteer>(MINIMAL_EVENT_COMPUTER_STEER, (event) => {
          const p = event.payload
          const text = p?.text ?? ''
          const attachments =
            p?.attachments && p.attachments.length > 0 ? p.attachments : undefined
          if (!text.trim() && !attachments) return
          const rec = useComputerRecorderStore.getState()
          if (rec.status === 'recording') return
          useComputerUseStore.getState().send(text, attachments)
        }),
      )
      register(
        await listen(MINIMAL_EVENT_COMPUTER_EXIT, () => {
          const cu = useComputerUseStore.getState()
          if (
            cu.status === 'running' ||
            cu.status === 'thinking' ||
            cu.status === 'connecting' ||
            cu.status === 'call_user'
          ) {
            cu.stop()
          }
          const rec = useComputerRecorderStore.getState()
          if (rec.status === 'recording') {
            rec.stopRecording()
          }
          useUIStore.getState().setAppMode('code')
        }),
      )
      register(
        await listen(MINIMAL_EVENT_COMPUTER_SYNC, () => {
          void emitComputerProgress(true)
        }),
      )
      register(useComputerUseStore.subscribe(() => void emitComputerProgress()))

      let lastRecorderKey = ''
      const emitRecorderProgress = async (force = false) => {
        const s = useComputerRecorderStore.getState()
        const last = s.steps.length > 0 ? s.steps[s.steps.length - 1] : null
        const payload: MinimalRecorderProgress = {
          status: s.status,
          error: s.error,
          statusMessage: s.statusMessage,
          stepCount: s.steps.length,
          lastActionType: last?.actionType ?? null,
          lastActionValue: last ? last.value || last.elementDescription || null : null,
          savedRecordingName: s.savedRecordingName,
          savedSkillName: s.savedSkillName,
          startedAt: s.startedAt,
        }
        const key = JSON.stringify(payload)
        if (!force && key === lastRecorderKey) return
        lastRecorderKey = key
        try {
          await emit(MINIMAL_EVENT_RECORDER_PROGRESS, payload)
        } catch {

        }
      }
      register(
        await listen<MinimalRecorderControl>(MINIMAL_EVENT_RECORDER_CONTROL, (event) => {
          const p = event.payload
          if (!p?.action) return
          const rec = useComputerRecorderStore.getState()
          switch (p.action) {
            case 'start': {
              const cu = useComputerUseStore.getState()
              if (
                cu.status === 'running' ||
                cu.status === 'thinking' ||
                cu.status === 'connecting' ||
                cu.status === 'call_user'
              ) {
                break
              }
              rec.setTask(p.task ?? '')
              rec.startRecording()
              break
            }
            case 'stop':
              rec.stopRecording()
              break
            case 'discard':
              rec.discardRecording()
              break
            case 'generate':
              rec.generateSkill()
              break
            case 'reset':
              rec.reset()
              break
          }
        }),
      )
      register(
        await listen<MinimalComputerReplay>(MINIMAL_EVENT_COMPUTER_REPLAY, (event) => {
          const p = event.payload
          if (!p?.name?.trim() || (p.mode !== 'smart' && p.mode !== 'exact')) return
          const cu = useComputerUseStore.getState()
          if (
            cu.status === 'running' ||
            cu.status === 'thinking' ||
            cu.status === 'connecting' ||
            cu.status === 'call_user'
          ) {
            return
          }
          const rec = useComputerRecorderStore.getState()
          if (rec.status === 'recording') {
            return
          }
          if (p.mode === 'exact') {
            cu.start({ replayRecording: p.name })
            return
          }
          if (p.provider && p.model) cu.setSelection(p.provider, p.model)
          const { provider, model } = useComputerUseStore.getState()
          if (p.useSkill) {
            if (!provider || !model) return
            cu.start({ skill: p.name, taskOverride: p.inputs ?? '' })
            return
          }
          if (!provider || !model) {
            cu.start({ replayRecording: p.name })
            useComputerUseStore.setState({
              error: translate(
                useSettingsStore.getState().locale,
                'computerUse.replay.smartFallback',
              ),
            })
            return
          }
          cu.start({ replayRecording: p.name, smart: true })
        }),
      )
      register(
        await listen(MINIMAL_EVENT_RECORDER_SYNC, () => {
          void emitRecorderProgress(true)
        }),
      )
      register(useComputerRecorderStore.subscribe(() => void emitRecorderProgress()))
    })()
    return () => {
      disposed = true
      for (const off of offs) off()
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
      ? t('app.launchingFailedDetail', { seconds: bootElapsedSecs })
      : t('app.launchingSlow', { seconds: bootElapsedSecs })
    return (
      <>
        <div
          className="app-window-frame items-center justify-center text-[var(--color-text-secondary)]"
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
      <DesignerDockCoordinator />
      <div
        className="app-window-frame outline-none ring-0"
      >
      <TitleBar />
      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
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
            {settingsOverlayOpen && (
              <div className="absolute inset-0 z-30 flex flex-col bg-[var(--color-surface)]">
                <Suspense fallback={null}>
                  <Settings />
                </Suspense>
              </div>
            )}
            {templateLibraryOpen && (
              <div className="absolute inset-0 z-30 flex flex-col bg-[var(--color-surface)]">
                <Suspense fallback={null}>
                  <TemplateLibrary />
                </Suspense>
              </div>
            )}
            {lanSharePanelOpen && (
              <div className="absolute inset-0 z-30 flex flex-col bg-[var(--color-surface)]">
                <Suspense fallback={null}>
                  <SharePanel />
                </Suspense>
              </div>
            )}
            {reviewPanelOpen && (
              <div className="absolute inset-0 z-30 flex flex-col bg-[var(--color-surface)]">
                <Suspense fallback={null}>
                  <ReviewPanel />
                </Suspense>
              </div>
            )}
          </div>
          {browserPanelVisible && (
            <>
              <ResizeHandleBrowser />
              <EmbeddedBrowserPanel />
            </>
          )}
          {designerCanvasVisible && (
            <>
              <ResizeHandleCanvas />
              <Suspense fallback={null}>
                <DesignerCanvasPanel />
              </Suspense>
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
      <TerminalPanel />
      {appMode === 'computer' && (
        <div className="absolute inset-0 z-40 flex flex-col bg-[var(--color-background)]">
          <ComputerUsePage />
        </div>
      )}
      </div>
      <StatusBar />
      <ToastContainer />
      <FileDragGhost />
      <BuddyCompanion />
      <UpdateChecker />
      <CodingModeTransitionGuard />
      <QuickModeSwitcher />
      <CommandPalette />
      {workspaceFinderMode && (
        <WorkspaceFinder
          mode={workspaceFinderMode}
          workDir={activeWorkDir}
          onClose={closeWorkspaceFinder}
        />
      )}
      <CloseChoiceModal />
      <SafeExitOverlay />
      </div>
      <ResizeHandles disabled={isMaximized} />
    </>
  )
}
