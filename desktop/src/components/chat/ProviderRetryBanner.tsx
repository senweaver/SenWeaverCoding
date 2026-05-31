// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useTranslation } from '../../i18n'

type Props = {
  sessionId: string
}

function formatCountdown(remainingMs: number): string {
  const secs = Math.max(0, Math.ceil(remainingMs / 1000))
  return `${secs}s`
}

function classLabel(klass: string, t: ReturnType<typeof useTranslation>): string {
  switch (klass) {
    case 'engine_overloaded':
      return t('chat.retry.engineOverloaded')
    case 'account_rate_limited':
      return t('chat.retry.accountRateLimited')
    case 'transient':
      return t('chat.retry.transient')
    default:
      return klass
  }
}

export function ProviderRetryBanner({ sessionId }: Props) {
  const t = useTranslation()
  const notice = useChatStore((s) => s.sessions[sessionId]?.providerRetry ?? null)
  const stopGeneration = useChatStore((s) => s.stopGeneration)
  const [now, setNow] = useState<number>(() => Date.now())

  useEffect(() => {
    if (!notice) return
    const id = window.setInterval(() => {
      setNow(Date.now())
    }, 250)
    return () => {
      window.clearInterval(id)
    }
  }, [notice?.receivedAt, notice?.attempt, notice])

  if (!notice) return null

  const remainingMs = Math.max(0, notice.waitDeadlineAt - now)
  const progressPct = notice.waitMs > 0
    ? Math.min(100, Math.max(0, 100 - (remainingMs / notice.waitMs) * 100))
    : 100

  const label = classLabel(notice.class, t)

  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="provider-retry-banner"
      data-session-id={sessionId}
      className="mb-3 flex w-full max-w-[860px] flex-col gap-1.5 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-900 dark:border-amber-400/40 dark:bg-amber-400/10 dark:text-amber-100"
    >
      <div className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className="size-2 flex-shrink-0 rounded-full bg-amber-500 animate-pulse"
        />
        <span className="font-medium">
          {t('chat.retry.title', { label })}
        </span>
        <span className="ml-auto flex items-center gap-2 text-xs opacity-80">
          <span>
            {t('chat.retry.attempt', {
              attempt: String(notice.attempt),
              max: String(notice.maxAttempts),
            })}
          </span>
          <span>·</span>
          <span>{formatCountdown(remainingMs)}</span>
          <button
            type="button"
            onClick={() => stopGeneration(sessionId)}
            className="ml-2 rounded border border-amber-500/60 px-2 py-0.5 text-xs font-medium text-amber-900 hover:bg-amber-500/20 dark:border-amber-400/60 dark:text-amber-100"
          >
            {t('chat.retry.stopButton')}
          </button>
        </span>
      </div>
      <div className="text-xs opacity-80">
        {notice.message}
        {notice.provider && (
          <span className="ml-2 opacity-60">
            ({notice.provider}/{notice.model})
          </span>
        )}
      </div>
      <div className="h-1 w-full overflow-hidden rounded-full bg-amber-500/20">
        <div
          className="h-full bg-amber-500/80 transition-[width] duration-200 ease-linear"
          style={{ width: `${progressPct}%` }}
        />
      </div>
    </div>
  )
}
