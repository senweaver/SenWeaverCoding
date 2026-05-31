// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { CodingModeSelector } from '../controls/CodingModeSelector'
import { ModelSelector } from '../controls/ModelSelector'
import { AttachmentGallery } from './AttachmentGallery'
import { TokenUsageRing } from './TokenUsageRing'
import { useSettingsStore } from '../../stores/settingsStore'
import { CODING_MODE_ACCENT } from '../../types/codingMode'
import type { AttachmentRef, UIAttachment } from '../../types/chat'

type Props = {

  sessionId: string

  initialContent: string

  initialAttachments?: UIAttachment[]

  onCancel: () => void

  onSubmit: (content: string, attachments: AttachmentRef[]) => void
}

export function InlineUserMessageEditor({
  sessionId,
  initialContent,
  initialAttachments,
  onCancel,
  onSubmit,
}: Props) {
  const t = useTranslation()
  const [text, setText] = useState(initialContent)
  const [sendButtonHover, setSendButtonHover] = useState(false)

  const attachments = initialAttachments ?? []
  const containerRef = useRef<HTMLDivElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const composingRef = useRef(false)
  const codingMode = useSettingsStore((s) => s.codingMode)
  const modeAccent = CODING_MODE_ACCENT[codingMode]

  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    requestAnimationFrame(() => {
      el.focus()
      const cursor = initialContent.length
      el.setSelectionRange(cursor, cursor)
    })
  }, [initialContent])

  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 240)}px`
  }, [text])

  useEffect(() => {
    function onMouseDown(e: MouseEvent) {
      const target = e.target as HTMLElement | null
      if (!target) return
      if (containerRef.current && containerRef.current.contains(target)) return

      const popover = target.closest('[role="menu"], [role="listbox"], [role="dialog"]')
      if (popover) return
      onCancel()
    }
    document.addEventListener('mousedown', onMouseDown, true)
    return () => document.removeEventListener('mousedown', onMouseDown, true)
  }, [onCancel])

  const handleSubmit = () => {
    const trimmed = text.trim()
    if (!trimmed && attachments.length === 0) return
    const refs: AttachmentRef[] = attachments.map((a) => ({
      type: a.type,
      name: a.name,
      data: a.data,
      mimeType: a.mimeType,
    }))
    onSubmit(trimmed, refs)
  }

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return
    if (event.key === 'Escape') {
      event.preventDefault()
      onCancel()
      return
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      handleSubmit()
    }
  }

  const canSubmit = text.trim().length > 0 || attachments.length > 0

  return (

    <div className="mb-5">
      <div
        ref={containerRef}
        className="flex w-full flex-col gap-2"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {attachments.length > 0 && (
          <AttachmentGallery attachments={attachments} variant="composer" />
        )}

        <div
          className="glass-panel relative flex min-h-[100px] flex-col rounded-xl px-3 py-2.5"

          onMouseDown={(e) => e.stopPropagation()}
        >
          <textarea
            ref={textareaRef}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={handleKeyDown}
            onCompositionStart={() => { composingRef.current = true }}
            onCompositionEnd={() => { composingRef.current = false }}
            rows={1}
            placeholder={t('chat.placeholder')}
            className="w-full min-h-[64px] resize-none bg-transparent py-1 pb-9 text-[13px] leading-relaxed text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)]"
          />

          <div className="absolute bottom-0 left-0 right-0 flex items-center justify-between px-3 py-2">
            <div className="flex items-center gap-1.5">
              <CodingModeSelector />
              <ModelSelector runtimeKey={sessionId} />
            </div>
            <div className="flex items-center gap-1">
              <TokenUsageRing sessionId={sessionId} size={14} />
              {(() => {
                const bgIdle = modeAccent?.accent ?? 'var(--color-text-primary)'
                const bgHover = modeAccent?.accentHover ?? 'var(--color-text-primary)'
                const fg = modeAccent?.onAccent ?? 'var(--color-surface)'
                const isDisabled = !canSubmit
                const bg = sendButtonHover && !isDisabled ? bgHover : bgIdle
                return (
                  <button
                    type="button"
                    onClick={handleSubmit}
                    onMouseEnter={() => setSendButtonHover(true)}
                    onMouseLeave={() => setSendButtonHover(false)}
                    disabled={isDisabled}
                    aria-label={t('common.send')}
                    title={t('common.send')}
                    className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full shadow-[var(--shadow-button-primary)] transition-all disabled:cursor-not-allowed disabled:opacity-30"
                    style={{ backgroundColor: bg, color: fg }}
                  >
                    <svg
                      width="14"
                      height="14"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2.6"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      aria-hidden
                    >
                      <line x1="12" y1="20" x2="12" y2="5" />
                      <polyline points="5,12 12,5 19,12" />
                    </svg>
                  </button>
                )
              })()}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
