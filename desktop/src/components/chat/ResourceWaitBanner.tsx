// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useTranslation } from '../../i18n'
import type {
  PendingResourceWait,
  ResourceWaitKind,
} from '../../stores/chatStore'

interface ResourceWaitBannerProps {
  sessionId: string
}

function shortPath(p: string): string {
  if (!p) return ''
  const norm = p.replace(/\\/g, '/')
  const parts = norm.split('/').filter(Boolean)
  if (parts.length <= 2) return norm
  return `…/${parts.slice(-2).join('/')}`
}

function kindLabel(
  t: ReturnType<typeof useTranslation>,
  kind: ResourceWaitKind,
  target: string,
): string {
  switch (kind) {
    case 'file':
      return t('chat.resourceWait.file', { path: shortPath(target) })
    case 'shell':
      return t('chat.resourceWait.shell')
    case 'browser':
      return t('chat.resourceWait.browser')
    default:
      return target
  }
}

function formatElapsed(startedAt: number, now: number): string {
  const sec = Math.max(0, Math.floor((now - startedAt) / 1000))
  if (sec < 60) return `${sec}s`
  const m = Math.floor(sec / 60)
  const s = sec % 60
  return `${m}m${s.toString().padStart(2, '0')}s`
}

export function ResourceWaitBanner({ sessionId }: ResourceWaitBannerProps) {
  const t = useTranslation()
  const waits = useChatStore(
    (s) => s.sessions[sessionId]?.pendingResourceWaits ?? [],
  )
  const sessions = useSessionStore((s) => s.sessions)
  const openTab = useTabStore((s) => s.openTab)
  const [expanded, setExpanded] = useState(false)
  const [tick, setTick] = useState(() => Date.now())

  const items = useMemo(() => waits, [waits])

  useEffect(() => {
    if (items.length === 0) return
    const id = setInterval(() => setTick(Date.now()), 1000)
    return () => clearInterval(id)
  }, [items.length])

  if (items.length === 0) return null

  const visible: PendingResourceWait[] = expanded ? items : items.slice(0, 1)
  const hidden = items.length - visible.length

  return (
    <div className="mx-auto w-full max-w-[860px] px-8 pt-2">
      <div className="rounded-lg border border-[var(--color-warning)]/30 bg-[var(--color-warning)]/8 px-3 py-2 text-[12px] text-[var(--color-text-primary)]">
        <div className="flex items-center gap-2 text-[11px] font-semibold text-[var(--color-warning)]">
          <span className="material-symbols-outlined animate-pulse text-[14px]">
            hourglass_top
          </span>
          <span>{t('chat.resourceWait.banner.title')}</span>
        </div>
        <div className="mt-1 flex flex-col gap-1">
          {visible.map((wait) => {
            const holderTitle =
              sessions.find((s) => s.id === wait.holderSessionId)?.title ||
              wait.holderTitle ||
              ''
            const heldByLabel = holderTitle
              ? t('chat.resourceWait.heldBy', { holder: holderTitle })
              : t('chat.resourceWait.heldByUnknown')
            return (
              <div
                key={wait.id}
                className="flex items-center gap-2 text-[11px] text-[var(--color-text-secondary)]"
                title={wait.kind === 'file' ? wait.target : undefined}
              >
                <span className="truncate">{kindLabel(t, wait.kind, wait.target)}</span>
                <span className="text-[var(--color-text-tertiary)]">·</span>
                <span className="truncate">{heldByLabel}</span>
                <span className="flex-shrink-0 rounded-full bg-[var(--color-warning)]/15 px-1.5 py-px font-mono text-[10px] tabular-nums text-[var(--color-warning)]">
                  {formatElapsed(wait.startedAt, tick)}
                </span>
                {wait.holderSessionId && wait.holderSessionId !== sessionId && (
                  <button
                    type="button"
                    className="ml-auto flex-shrink-0 text-[10px] font-medium text-[var(--color-brand)] hover:underline"
                    onClick={() =>
                      openTab(
                        wait.holderSessionId,
                        holderTitle || wait.holderSessionId,
                        'session',
                      )
                    }
                  >
                    {t('chat.resourceWait.switchTo')}
                  </button>
                )}
              </div>
            )
          })}
          {hidden > 0 && !expanded && (
            <button
              type="button"
              className="self-start text-[10px] font-medium text-[var(--color-brand)] hover:underline"
              onClick={() => setExpanded(true)}
            >
              {t('chat.resourceWait.expandMore', { count: hidden })}
            </button>
          )}
          {expanded && items.length > 1 && (
            <button
              type="button"
              className="self-start text-[10px] font-medium text-[var(--color-brand)] hover:underline"
              onClick={() => setExpanded(false)}
            >
              {t('chat.resourceWait.collapse')}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
