// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
import { useTranslation } from '../../../i18n'
import {
  extractPath,
  extractQuery,
  extractTextContent,
  meaningfulListPath,
  truncate,
} from '../../../utils/toolFormatters'

export function ListHeader({ input }: ToolViewProps) {
  const t = useTranslation()
  const rawPath = extractPath(input)
  const pattern = extractQuery(input)
  const path = meaningfulListPath(rawPath)
  const label = pattern || path
  const display = truncate(label ? label : t('tool.list.workspaceRoot'), 64)
  const title =
    pattern && path
      ? `${path} · ${pattern}`
      : pattern
        ? pattern
        : path
          ? path
          : t('tool.list.workspaceRoot')
  return (
    <span
      className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
      title={title}
    >
      {display}
    </span>
  )
}

export function ListDetail({ input, result }: ToolViewProps) {
  const t = useTranslation()
  const rawPath = extractPath(input)
  const pattern = extractQuery(input)
  const path = meaningfulListPath(rawPath)
  const text = result ? extractTextContent(result.content) : ''
  const isWorkspaceDot = rawPath.trim() === '.' || rawPath.trim() === './'
  const pathLine =
    pattern && path
      ? `${path} · ${pattern}`
      : pattern
        ? pattern
        : path
          ? path
          : isWorkspaceDot
            ? t('tool.list.workspaceRoot')
            : ''
  const copyMeta =
    pattern && path ? `${path}\n${pattern}` : pattern || path || (isWorkspaceDot ? t('tool.list.workspaceRoot') : '')
  const showMeta = Boolean(pattern || path || isWorkspaceDot)

  return (
    <div className="space-y-2">
      {showMeta && (
        <div className="flex items-center justify-between rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5">
          <span
            className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]"
            title={pathLine || undefined}
          >
            {pathLine}
          </span>
          <CopyButton
            text={copyMeta}
            className="ml-2 rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          />
        </div>
      )}
      {text ? (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <CodeViewer code={text} language="plaintext" maxLines={24} />
        </div>
      ) : (
        result &&
        !result.isError && (
          <div className="rounded-md border border-[var(--color-border)]/60 bg-[var(--color-surface-container-low)] px-3 py-2 text-[11px] text-[var(--color-text-tertiary)]">
            No output
          </div>
        )
      )}
    </div>
  )
}
