// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useUIStore, type Toast as ToastType } from '../../stores/uiStore'
import { useDockEdgeOffset } from '../../hooks/useDockEdgeOffset'
import { useTabStore } from '../../stores/tabStore'
import { useSessionStore } from '../../stores/sessionStore'
import { focusSession } from '../../lib/focusSession'
import { resolveSessionTitle } from '../../utils/sessionTitle'
import { useTranslation } from '../../i18n'

const typeStyles: Record<ToastType['type'], string> = {
  success: 'border-l-4 border-l-[var(--color-success)]',
  error: 'border-l-4 border-l-[var(--color-error)]',
  warning: 'border-l-4 border-l-[var(--color-warning)]',
  info: 'border-l-4 border-l-[var(--color-text-accent)]',
}

function ToastItem({ toast }: { toast: ToastType }) {
  const t = useTranslation()
  const removeToast = useUIStore((s) => s.removeToast)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const ownerSession = useSessionStore((s) =>
    toast.sessionId ? s.sessions.find((session) => session.id === toast.sessionId) ?? null : null,
  )

  const isCrossSession =
    Boolean(toast.sessionId) && toast.sessionId !== (activeTabId ?? undefined)
  const ownerLabel = ownerSession
    ? resolveSessionTitle(ownerSession.title, toast.sessionId ?? '')
    : toast.sessionId
  const displayMessage = isCrossSession
    ? `[${ownerLabel ?? toast.sessionId ?? ''}] ${toast.message}`
    : toast.message

  const hasActions = isCrossSession || Boolean(toast.action)

  return (
    <div
      className={`
        bg-[var(--color-surface)] rounded-[var(--radius-md)] shadow-[var(--shadow-dropdown)]
        px-3 py-2 text-[12px] text-[var(--color-text-primary)]
        ${typeStyles[toast.type]}
        animate-in slide-in-from-right fade-in duration-200
      `}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="flex-1">{displayMessage}</span>
        <button
          onClick={() => removeToast(toast.id)}
          className="text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] text-sm leading-none shrink-0"
        >
          ×
        </button>
      </div>
      {hasActions && (
        <div className="mt-1.5 flex flex-wrap justify-end gap-2">
          {isCrossSession && toast.sessionId && (
            <button
              type="button"
              onClick={() => {
                focusSession(toast.sessionId!)
                removeToast(toast.id)
              }}
              className="rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2.5 py-0.5 text-[11px] text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
            >
              {t('permission.switchToSession')}
            </button>
          )}
          {toast.action && (
            <button
              type="button"
              onClick={() => {
                toast.action?.onClick()
                removeToast(toast.id)
              }}
              className="rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2.5 py-0.5 text-[11px] text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
            >
              {toast.action.label}
            </button>
          )}
        </div>
      )}
    </div>
  )
}

export function ToastContainer() {
  const toasts = useUIStore((s) => s.toasts)
  const rightInset = useDockEdgeOffset()

  if (toasts.length === 0) return null

  return (
    <div
      className="fixed bottom-4 z-[100] flex flex-col gap-2 max-w-sm"
      style={{ right: `${rightInset + 16}px` }}
    >
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  )
}
