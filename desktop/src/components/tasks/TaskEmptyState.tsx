// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { Button } from '../shared/Button'
import { useTranslation } from '../../i18n'

type Props = {
  onCreateTask: () => void
}

export function TaskEmptyState({ onCreateTask }: Props) {
  const t = useTranslation()
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] py-20">
      <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-[var(--color-surface-info)]">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="var(--color-text-tertiary)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="10" />
          <polyline points="12 6 12 12 16 14" />
        </svg>
      </div>

      <h3 className="mb-1 text-xs font-semibold text-[var(--color-text-primary)]">
        {t('tasks.emptyTitle')}
      </h3>
      <p className="mb-4 max-w-sm text-center text-xs text-[var(--color-text-tertiary)]">
        {t('tasks.emptyDesc')}
      </p>

      <Button size="sm" onClick={onCreateTask}>{t('tasks.newTask')}</Button>
    </div>
  )
}
