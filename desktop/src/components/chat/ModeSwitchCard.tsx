// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'
import type { CodingModeId } from '../../types/codingMode'

type Props = {
  messageId: string
  planPath: string
  targetMode: CodingModeId
  status: 'pending' | 'switched' | 'dismissed'
  superseded?: boolean
  sessionId?: string | null
  handoffKind?: 'plan' | 'curator'
}

export function ModeSwitchCard({
  messageId,
  status,
  superseded,
  sessionId,
  planPath,
  handoffKind,
}: Props) {
  const t = useTranslation()
  const confirmModeSwitch = useChatStore((s) => s.confirmModeSwitch)
  const dismissModeSwitch = useChatStore((s) => s.dismissModeSwitch)
  const activeTabId = useTabStore((s) => s.activeTabId)

  useEffect(() => {
    if (status !== 'pending') return
    const handler = (e: KeyboardEvent) => {

      if (sessionId && activeTabId !== sessionId) return
      if (e.ctrlKey && e.key === 'Enter') {
        e.preventDefault()
        if (sessionId) confirmModeSwitch(sessionId, messageId)
      } else if (e.key === 'Escape') {
        e.preventDefault()
        if (sessionId) dismissModeSwitch(sessionId, messageId)
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [status, sessionId, messageId, confirmModeSwitch, dismissModeSwitch, activeTabId])

  const isPending = status === 'pending'

  const inferredKind: 'plan' | 'curator' =
    handoffKind ??
    (planPath && /impl_blueprint\.md$/i.test(planPath) ? 'curator' : 'plan')
  const isCurator = inferredKind === 'curator'

  const containerCls = isCurator
    ? 'rounded-[var(--radius-lg)] border border-[var(--color-curator-accent)]/55 ring-1 ring-[var(--color-curator-accent)]/25 shadow-[0_2px_18px_-8px_var(--color-curator-accent)] bg-[var(--color-surface-container-lowest)] overflow-hidden'
    : 'rounded-[var(--radius-lg)] border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] overflow-hidden'

  const iconCls = isCurator
    ? 'material-symbols-outlined text-[16px] text-[var(--color-curator-accent)]'
    : 'material-symbols-outlined text-[16px] text-[var(--color-text-secondary)]'

  const titleText = isCurator
    ? t('curator.modeSwitchTitle')
    : t('plan.modeSwitchTitle')

  const bodyText = isCurator
    ? t('curator.modeSwitchBody')
    : t('plan.modeSwitchBody')

  const switchBtnCls = isCurator
    ? 'flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-curator-accent)] text-white hover:bg-[var(--color-curator-accent-hover)] transition-all'
    : 'flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-text-primary)] text-[var(--color-surface)] hover:brightness-110 transition-all'

  return (
    <div
      className={`mb-3 ${superseded ? 'opacity-60 saturate-50 pointer-events-none' : ''}`}
    >
      <div className={containerCls}>
        <div className="flex items-center gap-2 px-3 py-2">
          <span className={iconCls}>
            swap_horiz
          </span>
          <span className="text-[12px] font-semibold text-[var(--color-text-primary)]">
            {titleText}
          </span>
          {status === 'switched' && (
            <span className="ml-auto inline-flex items-center gap-1 rounded-full bg-[var(--color-success)]/12 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-[var(--color-success)]">
              <span className="material-symbols-outlined text-[12px]">check</span>
              {t('plan.modeSwitchSwitched')}
            </span>
          )}
          {status === 'dismissed' && (
            <span className="ml-auto inline-flex items-center gap-1 rounded-full bg-[var(--color-surface-container)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-[var(--color-text-tertiary)]">
              {t('plan.modeSwitchDismissed')}
            </span>
          )}
        </div>
        <div className="px-3 pb-2 text-[12px] text-[var(--color-text-secondary)] leading-relaxed">
          {bodyText}
        </div>
        {isPending && (
          <div className="flex items-center justify-between gap-2 border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)] px-3 py-1.5">
            <span className="flex items-center gap-1 text-[11px] text-[var(--color-text-tertiary)]">
              {t('plan.modeSwitchAlwaysAsk')}
              <span className="material-symbols-outlined text-[12px]">expand_more</span>
            </span>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => sessionId && dismissModeSwitch(sessionId, messageId)}
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-2 py-1 text-[11px] font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] transition-colors"
              >
                {t('plan.skip')}
                <span className="text-[10px] text-[var(--color-text-tertiary)] tabular-nums px-1 py-0.5 rounded bg-[var(--color-surface-container)]">
                  {t('plan.skipKey')}
                </span>
              </button>
              <button
                type="button"
                onClick={() => sessionId && confirmModeSwitch(sessionId, messageId)}
                className={switchBtnCls}
              >
                {t('plan.modeSwitchSwitch')}
                <span className="text-[10px] px-1 py-0.5 rounded bg-[var(--color-text-secondary)]/20">
                  {t('plan.modeSwitchSwitchShortcut')}
                </span>
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
