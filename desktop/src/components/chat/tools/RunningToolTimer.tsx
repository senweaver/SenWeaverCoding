// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useRef } from 'react'
import { useFreshnessNow } from '../../../hooks/useFreshnessTicker'
import { useTranslation } from '../../../i18n'

export function formatElapsed(ms: number): string {
  const totalSec = Math.max(0, Math.floor(ms / 1000))
  if (totalSec < 60) return `${totalSec}s`
  const m = Math.floor(totalSec / 60)
  const s = totalSec % 60
  return `${m}m${s.toString().padStart(2, '0')}s`
}

function positiveNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null
}

function timeoutMsFor(toolName: string, input: unknown): number | null {
  const rec =
    input && typeof input === 'object' ? (input as Record<string, unknown>) : null
  const timeoutMs = positiveNumber(rec?.timeout_ms)
  if (timeoutMs !== null) return timeoutMs
  const timeoutSecs = positiveNumber(rec?.timeout_secs)
  if (timeoutSecs !== null) return timeoutSecs * 1000
  if (toolName === 'sleep') {
    const seconds = positiveNumber(rec?.seconds)
    if (seconds !== null) return seconds * 1000
  }
  if (toolName === 'background_wait') return 120_000
  return null
}

export function RunningToolTimer({
  toolName,
  input,
  startedAt,
}: {
  toolName: string
  input: unknown
  startedAt?: number
}) {
  const t = useTranslation()
  const now = useFreshnessNow(true)
  const fallbackStartRef = useRef(Date.now())
  const base =
    typeof startedAt === 'number' && startedAt > 0 ? startedAt : fallbackStartRef.current
  const timeoutMs = timeoutMsFor(toolName, input)
  const deadline = timeoutMs !== null ? base + timeoutMs : null
  const label =
    deadline !== null && deadline > now
      ? t('tool.running.remaining', { time: formatElapsed(deadline - now) })
      : formatElapsed(now - base)
  return (
    <span className="shrink-0 font-[var(--font-mono)] text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
      {label}
    </span>
  )
}
