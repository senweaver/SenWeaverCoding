// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef } from 'react'
import { ChatInput } from '../chat/ChatInput'
import { MINIMAL_EVENT_SUBMIT } from '../../lib/minimalMode'
import type { MinimalSubmitPayload } from '../../lib/minimalMode'
import { flushSessionRuntimeSelections } from '../../stores/sessionRuntimeStore'

type MinimalComposerProps = {
  onHeightChange: (height: number) => void
  onSubmitted: () => void
}

function occupiedHeight(root: HTMLElement): number {
  const card = root.getBoundingClientRect()
  let top = card.top
  let bottom = card.bottom
  root.querySelectorAll('#file-search-menu, #slash-command-menu, #local-slash-command-panel').forEach((node) => {
    const rect = node.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return
    top = Math.min(top, rect.top)
    bottom = Math.max(bottom, rect.bottom)
  })
  return Math.round(bottom - top)
}

export function MinimalComposer({ onHeightChange, onSubmitted }: MinimalComposerProps) {
  const wrapRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    let raf = 0
    const report = () => {
      if (raf) cancelAnimationFrame(raf)
      raf = requestAnimationFrame(() => {
        raf = 0
        const h = occupiedHeight(el)
        if (h > 0) onHeightChange(h)
      })
    }
    report()
    const ro = new ResizeObserver(report)
    ro.observe(el)
    const mo = new MutationObserver(report)
    mo.observe(el, { childList: true, subtree: true })
    return () => {
      if (raf) cancelAnimationFrame(raf)
      ro.disconnect()
      mo.disconnect()
    }
  }, [onHeightChange])

  const handleSubmit = useCallback(
    (sessionId: string, content: string, attachments?: MinimalSubmitPayload['attachments'], options?: MinimalSubmitPayload['options']) => {
      if (!sessionId) return
      flushSessionRuntimeSelections()
      const payload: MinimalSubmitPayload = { sessionId, content, attachments, options }
      void (async () => {
        try {
          const { emit } = await import('@tauri-apps/api/event')
          await emit(MINIMAL_EVENT_SUBMIT, payload)
        } catch (err) {
          console.warn('[minimal] forward submit failed', err)
        }
      })()
      onSubmitted()
    },
    [onSubmitted],
  )

  return (
    <div
      ref={wrapRef}
      className="min-w-0 w-full max-w-full overflow-visible rounded-2xl border border-white/50 bg-[var(--color-surface)]/95 shadow-[0_10px_40px_rgba(30,58,95,0.28)] backdrop-blur-md [&>*]:min-w-0 [&>*]:max-w-full"
      data-minimal-composer
    >
      <ChatInput variant="default" embedded onSubmit={handleSubmit} />
    </div>
  )
}
