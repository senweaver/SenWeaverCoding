// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect } from 'react'
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

export function MinimalInputWindow() {
  useMinimalWindowBridge()

  const variant = useMinimalStore((s) => s.variant)
  const opacityPct = useMinimalStore((s) => s.opacityPct)
  const activeTabId = useTabStore((s) => s.activeTabId)

  const hideSelf = useCallback(async () => {
    if (!isTauriRuntime()) return
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      if (!(await win.isVisible())) return
      await win.hide()
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
            window.setTimeout(() => {
              void revealMinimalInputWindow(target)
            }, 0)
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
      className="flex h-screen w-screen flex-col justify-end overflow-hidden px-4 pt-4 pb-2"
      style={{ opacity: opacityPct / 100 }}
    >
      <div
        className={
          variant === 'computer'
            ? 'min-h-0 max-h-full overflow-y-auto -m-3 p-3'
            : 'hidden'
        }
      >
        <ComputerPanel onHeightChange={noopHeight} onSubmitted={handleSubmitted} />
      </div>
      <div
        className={
          variant === 'computer'
            ? 'hidden'
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
