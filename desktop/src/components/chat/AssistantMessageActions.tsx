// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from '../../i18n'
import { copyTextToClipboard } from './clipboard'
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown'
import { useSessionStore } from '../../stores/sessionStore'
import { useTabStore } from '../../stores/tabStore'
import { useUIStore } from '../../stores/uiStore'

type Props = {
  copyText: string
  sessionId?: string | null
  workDir?: string | null
  disableFork?: boolean
}

export function AssistantMessageActions({ copyText, sessionId, workDir, disableFork }: Props) {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)
  const [open, setOpen] = useState(false)
  const { triggerRef, menuRef, style, portalTarget } = useAnchoredDropdown<HTMLButtonElement>(
    open,
    () => setOpen(false),
    { align: 'right', estimatedHeight: 110 },
  )

  const copyMessage = useCallback(async () => {
    const ok = await copyTextToClipboard(copyText)
    if (ok) addToast({ type: 'success', message: t('chat.copyMessageToast') })
    else addToast({ type: 'error', message: t('chat.copyFailedToast') })
    setOpen(false)
  }, [addToast, copyText, t])

  const forkChat = useCallback(async () => {
    if (disableFork || !sessionId?.trim()) return
    try {
      const newId = await useSessionStore.getState().createSession(workDir?.trim() || undefined)
      useTabStore.getState().openTab(newId, t('sidebar.newSession'))
      addToast({ type: 'success', message: t('chat.forkChatToast') })
    } catch (e) {
      addToast({ type: 'error', message: e instanceof Error ? e.message : String(e) })
    }
    setOpen(false)
  }, [addToast, disableFork, sessionId, t, workDir])

  const forkDisabled = Boolean(disableFork || !sessionId?.trim())

  const itemClass =
    'flex w-full px-3 py-2.5 text-left text-[13px] text-[var(--color-text-primary)] transition-colors hover:bg-[var(--color-surface-hover)] disabled:cursor-not-allowed disabled:opacity-45'

  return (
    <div className="relative flex w-full justify-end">
      <button
        ref={triggerRef}
        type="button"
        className="inline-flex h-5 w-5 items-center justify-center rounded-full text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-brand)]/35"
        aria-label={t('chat.messageMoreActions')}
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen((v) => !v)}
      >
        {}
        <span
          className="material-symbols-outlined text-[12px] leading-none"
          style={{ fontVariationSettings: "'wght' 300, 'opsz' 20, 'GRAD' 0, 'FILL' 0" }}
          aria-hidden="true"
        >
          more_horiz
        </span>
      </button>
      {open && style && createPortal(
        <div
          ref={menuRef}
          role="menu"
          style={style}
          className="min-w-[160px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-[0_18px_40px_-18px_rgba(0,0,0,0.45)]"
        >
          <button type="button" role="menuitem" className={itemClass} disabled={forkDisabled} onClick={() => void forkChat()}>
            {t('chat.forkChat')}
          </button>
          <button type="button" role="menuitem" className={itemClass} onClick={() => void copyMessage()}>
            {t('chat.copyMessage')}
          </button>
        </div>,
        portalTarget,
      )}
    </div>
  )
}
