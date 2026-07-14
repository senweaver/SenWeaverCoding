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
  prewarmMinimalInputWindow,
  revealMinimalInputWindow,
} from './lib/minimalMode'
import type { MinimalVariant } from './lib/minimalMode'

const noopHeight = () => {}

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

  const hideSelf = useCallback(async (opts?: { silent?: boolean }) => {
    if (!isTauriRuntime()) return
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
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        const off = await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
          if (!focused) {
            if (prewarmingRef.current) return
            void hideSelf()
            return
          }
          requestAnimationFrame(() => {
            document
              .querySelector<HTMLElement>('[data-role="chat-composer"], textarea')
              ?.focus()
          })
        })
        if (disposed) off()
        else unlisten = off
      } catch {

      }
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [hideSelf])

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
              await revealMinimalInputWindow(target)
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
  }, [])

  const handleSubmitted = useCallback(() => {
    void hideSelf()
  }, [hideSelf])

  return (
    <div
      className="relative flex h-screen w-screen flex-col justify-end overflow-hidden px-4 pt-4 pb-2"
      style={{ opacity: opacityPct / 100 }}
    >
      <div
        aria-hidden={variant !== 'computer'}
        className={
          variant === 'computer'
            ? 'min-h-0 max-h-full overflow-y-auto -m-3 p-3'
            : 'pointer-events-none absolute bottom-2 left-4 right-4 -z-10 opacity-0'
        }
      >
        <ComputerPanel onHeightChange={noopHeight} onSubmitted={handleSubmitted} />
      </div>
      <div
        aria-hidden={variant === 'computer'}
        className={
          variant === 'computer'
            ? 'pointer-events-none absolute bottom-2 left-4 right-4 -z-10 opacity-0'
            : 'min-h-0 max-h-full overflow-y-auto -m-3 p-3'
        }
      >
        {activeTabId ? (
          <MinimalComposer onHeightChange={noopHeight} onSubmitted={handleSubmitted} />
        ) : (
          <NoSessionHint />
        )}
      </div>
    </div>
  )
}
