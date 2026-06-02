// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useTranslation } from '../../i18n'
import {
  isWorkspaceRootPath,
  splitPathForDisplay,
} from '../../utils/toolFormatters'
import { CopyButton } from '../shared/CopyButton'

type Props = {
  path: string
  additions: number
  deletions: number
  diff?: string | null
  editBatchId?: string | null
}

export function FileEditNotification({
  path,
  additions,
  deletions,
  diff,
  editBatchId,
}: Props) {
  const t = useTranslation()
  const workspaceRoot = isWorkspaceRootPath(path)
  const { dir, tail, separator } = splitPathForDisplay(path)
  const hasDiff = typeof diff === 'string' && diff.trim().length > 0
  const [open, setOpen] = useState(false)

  return (
    <div
      className="mb-2 rounded-lg border border-[var(--color-border)]/50 bg-[var(--color-surface-container-lowest)]"
      title={path}
    >
      <div className="flex items-center gap-2 px-3 py-1.5">
        <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
          edit_note
        </span>
        {workspaceRoot ? (
          <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px] italic text-[var(--color-text-tertiary)]">
            {t('tool.list.workspaceRoot')}
          </span>
        ) : dir ? (
          <span className="min-w-0 flex-1 flex items-baseline truncate">
            <span className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
              {dir}
              {separator}
            </span>
            <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-primary)]">
              {tail}
            </span>
          </span>
        ) : (
          <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-primary)]">
            {tail || path || 'file'}
          </span>
        )}
        {additions > 0 && (
          <span className="shrink-0 rounded-full bg-[var(--color-success-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-success)]">
            +{additions}
          </span>
        )}
        {deletions > 0 && (
          <span className="shrink-0 rounded-full bg-[var(--color-error-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-error)]">
            -{deletions}
          </span>
        )}
        {editBatchId && (
          <span
            className="shrink-0 rounded-full border border-[var(--color-border)] px-1.5 py-0.5 font-[var(--font-mono)] text-[9px] text-[var(--color-text-tertiary)]"
            title={`${t('tool.batchId')}: ${editBatchId}`}
          >
            #{editBatchId.slice(0, 6)}
          </span>
        )}
        {hasDiff && (
          <button
            type="button"
            onClick={() => setOpen((value) => !value)}
            className="shrink-0 rounded-md border border-[var(--color-border)] px-1.5 py-0.5 text-[10px] uppercase tracking-[0.14em] text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
            aria-expanded={open}
          >
            {open ? t('tool.edit.hideDiff') : t('tool.edit.showDiff')}
          </button>
        )}
      </div>
      {hasDiff && open && (
        <div className="border-t border-[var(--color-border)]/40 bg-[var(--color-surface-container-low)]">
          <div className="flex items-center justify-between px-3 py-1 text-[10px] uppercase tracking-[0.14em] text-[var(--color-text-tertiary)]">
            <span>diff</span>
            <CopyButton
              text={diff!}
              className="rounded-md border border-[var(--color-outline-variant)]/40 px-2 py-0.5 text-[10px] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <DiffLines diff={diff!} />
        </div>
      )}
    </div>
  )
}

function DiffLines({ diff }: { diff: string }) {
  const lines = diff.replace(/\n$/, '').split('\n')
  return (
    <div
      className="m-0 max-h-[260px] overflow-auto bg-[var(--color-code-bg)] py-1 font-[var(--font-mono)] text-[11px] leading-[1.5]"
      style={{ whiteSpace: 'pre' }}
    >
      {lines.map((line, index) => {
        const first = line.charAt(0)
        const isFileHeader = line.startsWith('---') || line.startsWith('+++')
        const isHunk = line.startsWith('@@')
        const isAdd = !isFileHeader && first === '+'
        const isDel = !isFileHeader && first === '-'

        let lineClass =
          'px-3 text-[var(--color-code-fg)]'
        if (isHunk) {
          lineClass =
            'px-3 text-[var(--color-text-tertiary)] bg-[var(--color-surface-container-high)]/40'
        } else if (isFileHeader) {
          lineClass = 'px-3 text-[var(--color-text-tertiary)]'
        } else if (isAdd) {
          lineClass =
            'px-3 text-[var(--color-success)] bg-[var(--color-success-container)]/30'
        } else if (isDel) {
          lineClass =
            'px-3 text-[var(--color-error)] bg-[var(--color-error-container)]/30'
        }

        return (
          <div key={index} className={lineClass}>
            {line.length > 0 ? line : '\u00a0'}
          </div>
        )
      })}
    </div>
  )
}
