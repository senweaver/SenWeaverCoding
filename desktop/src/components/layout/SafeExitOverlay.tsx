// SPDX-License-Identifier: MIT

import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useSessionRunStateStore } from '../../stores/sessionRunStateStore'
import { forceQuit } from '../../lib/appClose'

export function SafeExitOverlay() {
  const t = useTranslation()
  const safeExiting = useUIStore((s) => s.safeExiting)
  const remaining = useSessionRunStateStore((s) => s.running.size)

  if (!safeExiting) return null

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-[var(--color-overlay-scrim)]">
      <div className="glass-panel flex w-[360px] max-w-[calc(100vw-48px)] flex-col items-center gap-4 rounded-[var(--radius-xl)] px-6 py-8 text-center">
        <span className="material-symbols-outlined animate-spin text-[32px] text-[var(--color-brand)]">
          progress_activity
        </span>
        <div>
          <div className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('close.saving.title')}
          </div>
          <p className="mt-1 text-xs text-[var(--color-text-tertiary)]">{t('close.saving.desc')}</p>
        </div>
        {remaining > 0 && (
          <div className="text-xs text-[var(--color-text-secondary)]">
            {t('close.saving.remaining', { count: remaining })}
          </div>
        )}
        <button
          type="button"
          onClick={() => void forceQuit()}
          className="mt-1 rounded-lg border border-[var(--color-border)] px-4 py-2 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
        >
          {t('close.saving.forceQuit')}
        </button>
      </div>
    </div>
  )
}
