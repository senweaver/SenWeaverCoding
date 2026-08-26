// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef } from 'react'
import { useTranslation } from './i18n'
import { useTabStore } from './stores/tabStore'
import { useMinimalStore } from './stores/minimalStore'
import { useMinimalWindowBridge } from './hooks/useMinimalWindowBridge'
import { MinimalComposer } from './components/minimal/MinimalComposer'
import { ComputerPanel } from './components/minimal/computer/ComputerPanel'
import { isTauriRuntime } from './lib/desktopRuntime'
import {
  MINIMAL_EVENT_INPUT_HIDDEN,
  MINIMAL_EVENT_INPUT_SHOW,
  MINIMAL_INPUT_SIZE,
  activateMinimalInputWindow,
  minimalInputShouldStayVisible,
  prewarmMinimalInputWindow,
  resizeMinimalWindow,
  revealMinimalInputWindow,
  setMinimalInputKeepVisible,
} from './lib/minimalMode'
import type { MinimalVariant } from './lib/minimalMode'

const INPUT_CHROME_Y = 32

function NoSessionHint() {
  const t = useTranslation()
  return (
    <div className="flex items-center gap-2 rounded-2xl border border-white/50 bg-[var(--color-surface)]/95 px-3 py-2.5 shadow-[0_10px_40px_rgba(30,58,95,0.28)] backdrop-blur-md">
      <span className="material-symbols-outlined text-[18px] text-[var(--color-text-tertiary)]">
        info
      </span>
      <span className="text-[12px] leading-snug text-[var(--color-text-secondary)]">
        {t('minimal.noSession')}
      </span>
    </div>
  )
}

function waitForPaint(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve())
    })
  })
}

export function MinimalInputWindow() {
  useMinimalWindowBridge()

  const variant = useMinimalStore((s) => s.variant)
  const opacityPct = useMinimalStore((s) => s.opacityPct)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const prewarmingRef = useRef(false)
  const revealSeqRef = useRef(0)
  const variantRef = useRef(variant)
  variantRef.current = variant
  const keepVisibleRef = useRef(false)
  const blurHideTimerRef = useRef<number | null>(null)
  const lastWindowHeightRef = useRef(0)
  const dropHoldUntilRef = useRef(0)

  const focusVisibleComposer = useCallback(() => {
    requestAnimationFrame(() => {
      const selector =
        variantRef.current === 'computer'
          ? '[data-minimal-pane="computer"] textarea'
          : '[data-minimal-pane="code"] [data-role="chat-composer"]'
      document.querySelector<HTMLElement>(selector)?.focus()
    })
  }, [])

  const applyKeepVisible = useCallback(async (keep: boolean) => {
    keepVisibleRef.current = keep
    await setMinimalInputKeepVisible(keep)
  }, [])

  const hideSelf = useCallback(async (opts?: { silent?: boolean }) => {
    if (!isTauriRuntime()) return
    if (blurHideTimerRef.current != null) {
      window.clearTimeout(blurHideTimerRef.current)
      blurHideTimerRef.current = null
    }
    await applyKeepVisible(false)
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      if (!(await win.isVisible())) return
      await win.hide()
      if (opts?.silent) return
      const { emit } = await import('@tauri-apps/api/event')
      await emit(MINIMAL_EVENT_INPUT_HIDDEN)
    } catch (err) {
      console.warn('[minimal-input] hide failed', err)
    }
  }, [applyKeepVisible])

  const applyContentHeight = useCallback((contentHeight: number) => {
    const size = MINIMAL_INPUT_SIZE[variantRef.current] ?? MINIMAL_INPUT_SIZE.code
    const nextH = Math.max(size.height, Math.round(contentHeight) + INPUT_CHROME_Y)
    if (nextH === lastWindowHeightRef.current) return
    lastWindowHeightRef.current = nextH
    void resizeMinimalWindow(size.width, nextH)
  }, [])

  const handleCodeHeight = useCallback((height: number) => {
    if (variantRef.current === 'computer') return
    applyContentHeight(height)
  }, [applyContentHeight])

  const handleComputerHeight = useCallback((height: number) => {
    if (variantRef.current !== 'computer') return
    applyContentHeight(height)
  }, [applyContentHeight])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        const win = getCurrentWindow()
        const off = await win.onFocusChanged(({ payload: focused }) => {
          if (blurHideTimerRef.current != null) {
            window.clearTimeout(blurHideTimerRef.current)
            blurHideTimerRef.current = null
          }
          if (focused) {
            focusVisibleComposer()
            return
          }
          if (prewarmingRef.current || keepVisibleRef.current) return
          if (Date.now() < dropHoldUntilRef.current) return
          blurHideTimerRef.current = window.setTimeout(() => {
            blurHideTimerRef.current = null
            void (async () => {
              if (prewarmingRef.current || keepVisibleRef.current) return
              if (Date.now() < dropHoldUntilRef.current) return
              if (await minimalInputShouldStayVisible()) return
              try {
                if (await win.isFocused()) return
              } catch {

              }
              void hideSelf()
            })()
          }, 80)
        })
        if (disposed) off()
        else unlisten = off
      } catch {

      }
    })()
    return () => {
      disposed = true
      if (blurHideTimerRef.current != null) {
        window.clearTimeout(blurHideTimerRef.current)
        blurHideTimerRef.current = null
      }
      if (unlisten) unlisten()
    }
  }, [hideSelf, focusVisibleComposer])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let cancelled = false
    let unlisten: (() => void) | null = null
    void (async () => {
      try {
        const { getCurrentWebview } = await import('@tauri-apps/api/webview')
        const webview = getCurrentWebview()
        const off = await webview.onDragDropEvent((event) => {
          const payload = event.payload as
            | { type: 'enter'; paths: string[]; position: { x: number; y: number } }
            | { type: 'over'; position: { x: number; y: number } }
            | { type: 'drop'; paths: string[]; position: { x: number; y: number } }
            | { type: 'leave' }
          if (payload.type === 'enter' || payload.type === 'over') {
            void applyKeepVisible(true)
            return
          }
          if (payload.type === 'drop') {
            dropHoldUntilRef.current = Date.now() + 800
            void (async () => {
              await activateMinimalInputWindow()
              await applyKeepVisible(false)
              focusVisibleComposer()
              dropHoldUntilRef.current = Date.now() + 400
            })()
            return
          }
          void (async () => {
            if (Date.now() < dropHoldUntilRef.current) return
            await applyKeepVisible(false)
            if (await minimalInputShouldStayVisible()) return
            try {
              const { getCurrentWindow } = await import('@tauri-apps/api/window')
              if (await getCurrentWindow().isFocused()) return
            } catch {

            }
            void hideSelf()
          })()
        })
        if (cancelled) {
          off()
        } else {
          unlisten = off
        }
      } catch {

      }
    })()
    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
  }, [applyKeepVisible, focusVisibleComposer, hideSelf])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      if (event.defaultPrevented) return
      void hideSelf()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [hideSelf])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let cancelled = false
    void (async () => {
      await waitForPaint()
      if (cancelled) return
      prewarmingRef.current = true
      try {
        await prewarmMinimalInputWindow()
        await new Promise((r) => window.setTimeout(r, 160))
      } finally {
        if (!cancelled) prewarmingRef.current = false
      }
    })()
    return () => {
      cancelled = true
      prewarmingRef.current = false
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event')
        const off = await listen<{ variant?: MinimalVariant }>(
          MINIMAL_EVENT_INPUT_SHOW,
          (event) => {
            const target: MinimalVariant =
              event.payload?.variant === 'computer' ? 'computer' : 'code'
            useMinimalStore.getState().setVariant(target)
            const seq = ++revealSeqRef.current
            void (async () => {
              await waitForPaint()
              if (disposed || seq !== revealSeqRef.current) return
              lastWindowHeightRef.current = 0
              await revealMinimalInputWindow(target)
              focusVisibleComposer()
            })()
          },
        )
        if (disposed) off()
        else unlisten = off
      } catch {

      }
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [focusVisibleComposer])

  const handleSubmitted = useCallback(() => {
    void hideSelf()
  }, [hideSelf])

  return (
    <div
      className="relative flex h-full w-full min-h-0 min-w-0 flex-col justify-end overflow-hidden p-4"
      style={{ opacity: opacityPct / 100 }}
    >
      <div
        data-minimal-pane="computer"
        aria-hidden={variant !== 'computer'}
        className={
          variant === 'computer'
            ? 'min-h-0 min-w-0 w-full max-h-full overflow-visible -m-3 p-3'
            : 'pointer-events-none absolute bottom-2 left-4 right-4 -z-10 opacity-0'
        }
      >
        <ComputerPanel onHeightChange={handleComputerHeight} onSubmitted={handleSubmitted} />
      </div>
      <div
        data-minimal-pane="code"
        aria-hidden={variant === 'computer'}
        className={
          variant === 'computer'
            ? 'pointer-events-none absolute bottom-2 left-4 right-4 -z-10 opacity-0'
            : 'min-h-0 min-w-0 w-full max-h-full overflow-visible -m-3 p-3'
        }
      >
        {activeTabId ? (
          <MinimalComposer onHeightChange={handleCodeHeight} onSubmitted={handleSubmitted} />
        ) : (
          <NoSessionHint />
        )}
      </div>
    </div>
  )
}
