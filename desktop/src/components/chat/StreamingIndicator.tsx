// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
  const elapsedSeconds = useChatStore((s) =>
    sessionId ? s.sessions[sessionId]?.elapsedSeconds ?? 0 : 0,
  )

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
      {elapsedSeconds > 0 && (
        <span className="font-[var(--font-mono)] text-[11px] tabular-nums">
          {formatElapsed(elapsedSeconds * 1000)}
        </span>
      )}
    </div>
  )
}
