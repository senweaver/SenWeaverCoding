// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useRef } from 'react'
import { useFreshnessNow } from '../../hooks/useFreshnessTicker'
import { useTranslation } from '../../i18n'
import type { TranslationKey } from '../../i18n'
import { useChatStore } from '../../stores/chatStore'
import { formatElapsed } from './tools/RunningToolTimer'

type Translate = (
  key: TranslationKey,
  params?: Record<string, string | number>,
) => string

export function formatPlanningLabel(action: string, t: Translate): string {
  if (action.trim().toLowerCase() === 'thinking') {
    return t('chat.thinkingNow')
  }
  return 'Planning next moves'
}

export function StreamingIndicator({
  action = '',
  sessionId,
}: {
  action?: string
  sessionId?: string | null
}) {
  const t = useTranslation()
  const label = formatPlanningLabel(action, t)
  const phaseStartedAt = useChatStore((s) =>
    sessionId ? s.sessions[sessionId]?.planningPhaseStartedAt ?? null : null,
  )
  const now = useFreshnessNow(true)
  const fallbackStartRef = useRef(Date.now())
  const elapsedMs = now - (phaseStartedAt ?? fallbackStartRef.current)

  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="planning-indicator"
      className="mb-3 flex w-fit items-center gap-2 px-1 py-0.5 text-[var(--color-text-tertiary)]"
    >
      <span
        aria-hidden="true"
        className="size-1.5 flex-shrink-0 rounded-full bg-[var(--color-text-tertiary)] animate-pulse"
      />
      <span className="text-sm italic">{label}</span>
      {elapsedMs >= 1000 && (
        <span className="font-[var(--font-mono)] text-[11px] tabular-nums">
          {formatElapsed(elapsedMs)}
        </span>
      )}
    </div>
  )
}
