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

export function MinimalComposer({ onHeightChange, onSubmitted }: MinimalComposerProps) {
  const wrapRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const report = () => {
      const h = Math.round(el.getBoundingClientRect().height)
      if (h > 0) onHeightChange(h)
    }
    report()
    const ro = new ResizeObserver(report)
    ro.observe(el)
    return () => ro.disconnect()
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
      className="overflow-hidden rounded-2xl border border-white/50 bg-[var(--color-surface)]/95 shadow-[0_10px_40px_rgba(30,58,95,0.28)] backdrop-blur-md"
      data-minimal-composer
    >
      <ChatInput variant="default" onSubmit={handleSubmit} />
    </div>
  )
}
