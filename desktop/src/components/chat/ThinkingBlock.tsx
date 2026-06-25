// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useEffect, useRef, useMemo } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useTranslation } from '../../i18n'
import { useChatStore } from '../../stores/chatStore'

type Props = {
  content: string
  isActive?: boolean
  startedAt?: number
  completedAt?: number

  defaultExpanded?: boolean

  compact?: boolean
}

export function ThinkingBlock({
  content,
  isActive = false,
  startedAt,
  completedAt,
  defaultExpanded = false,
  compact = false,
}: Props) {
  const t = useTranslation()
  const [userOverride, setUserOverride] = useState<boolean | null>(null)
  const [elapsedSec, setElapsedSec] = useState(0)
  const [isContentScrollable, setIsContentScrollable] = useState(false)
  const contentRef = useRef<HTMLDivElement>(null)
  const prevIsActiveRef = useRef<boolean>(isActive)

  const expanded =
    userOverride !== null ? userOverride : isActive || defaultExpanded

  const completedSeconds = useMemo(() => {
    if (!startedAt || !completedAt || completedAt < startedAt) return null
    return Math.max(1, Math.round((completedAt - startedAt) / 1000))
  }, [startedAt, completedAt])

  useEffect(() => {
    if (prevIsActiveRef.current !== isActive) {
      setUserOverride(null)
      prevIsActiveRef.current = isActive
    }
  }, [isActive])

  useEffect(() => {
    if (!isActive || startedAt == null) {
      setElapsedSec(0)
      return
    }
    const tick = () => {
      setElapsedSec(Math.max(1, Math.floor((Date.now() - startedAt) / 1000)))
    }
    tick()
    const id = window.setInterval(tick, 1000)
    return () => window.clearInterval(id)
  }, [isActive, startedAt])

  useEffect(() => {
    const el = contentRef.current
    if (!el || !expanded) {
      if (isContentScrollable) setIsContentScrollable(false)
      return
    }
    if (isActive) {
      el.scrollTop = el.scrollHeight
    }
    const scrollable = el.scrollHeight - el.clientHeight > 1
    setIsContentScrollable((prev) => (prev === scrollable ? prev : scrollable))
  }, [content, expanded, isActive, isContentScrollable])

  const lines = content.split('\n').filter((l) => l.trim())
  const firstLine = lines[0]?.replace(/\s+/g, ' ').trim() || ''
  const preview = firstLine.length > 80 ? firstLine.slice(0, 80) + '...' : firstLine

  const durationSeconds =
    isActive ? (startedAt != null ? elapsedSec : null) : completedSeconds

  const hasContent = content.trim().length > 0
  const canExpand = hasContent
  const showInlinePreview = (compact || isActive) && !expanded
  const contentSizeClass = isActive
    ? compact
      ? 'max-h-[160px]'
      : 'max-h-[240px]'
    : 'max-h-[320px]'
  const activeScrollMask =
    isActive && isContentScrollable
      ? {
          maskImage: 'linear-gradient(to bottom, transparent 0, #000 16px)',
          WebkitMaskImage: 'linear-gradient(to bottom, transparent 0, #000 16px)',
        }
      : undefined

  return (
    <div className={compact ? 'mb-1' : 'mb-1.5'}>
      <style>{thinkingStyles}</style>
      <button
        type="button"
        onClick={() => {
          if (canExpand) setUserOverride(!expanded)
        }}
        aria-expanded={expanded}
        disabled={!canExpand}
        className={`flex w-full items-center gap-2 rounded-md px-1 py-0.5 text-left text-[12px] transition-colors ${
          canExpand
            ? 'cursor-pointer hover:bg-[var(--color-surface-hover)]/35'
            : 'cursor-default'
        }`}
      >
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <div className="flex min-w-0 shrink items-center gap-1">
            <span className="shrink-0 font-medium leading-snug tracking-tight text-[var(--color-text-secondary)]">
              {isActive ? t('thinking.wordThinking') : t('thinking.wordThought')}
            </span>
            {durationSeconds != null && (
              <span className="shrink-0 font-normal leading-snug tracking-tight text-[var(--color-text-tertiary)]">
                {t('thinking.durationForSeconds', { seconds: durationSeconds })}
              </span>
            )}
            {isActive && <span className="thinking-dots shrink-0" />}
          </div>

          {canExpand && (
            <span className="material-symbols-outlined shrink-0 text-[16px] text-[var(--color-outline)]" aria-hidden>
              {expanded ? 'expand_less' : 'chevron_right'}
            </span>
          )}

          {showInlinePreview && preview && (
            <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
              {preview}
              {}
            </span>
          )}
        </div>
      </button>
      {expanded && canExpand && (
        <div
          ref={contentRef}
          style={activeScrollMask}
          className={`mt-1 ${contentSizeClass} overflow-y-auto rounded-lg border border-[var(--color-border)]/40 bg-[var(--color-surface-container-lowest)] px-2.5 py-2 font-[var(--font-mono)] text-[11px] leading-[1.45] whitespace-pre-wrap break-words text-[var(--color-text-secondary)]`}
        >
          {content}
        </div>
      )}
    </div>
  )
}

export function ActiveThinkingBlock({
  sessionId,
  compact,
  onContentGrow,
}: {
  sessionId: string | null
  compact?: boolean
  onContentGrow?: () => void
}) {
  const { content, startedAt } = useChatStore(
    useShallow((s) => {
      const st = sessionId ? s.sessions[sessionId] : undefined
      return {
        content: st?.activeThinkingContent ?? '',
        startedAt: st?.activeThinkingStartedAt ?? null,
      }
    }),
  )
  useEffect(() => {
    onContentGrow?.()
  }, [content, onContentGrow])
  return (
    <ThinkingBlock
      content={content}
      isActive
      startedAt={startedAt ?? undefined}
      compact={compact}
    />
  )
}

const thinkingStyles = `
@keyframes thinking-dots {
  0%, 20% { content: ''; }
  40% { content: '.'; }
  60% { content: '..'; }
  80%, 100% { content: '...'; }
}
.thinking-dots::after {
  content: '';
  animation: thinking-dots 1.4s steps(1, end) infinite;
}
`
