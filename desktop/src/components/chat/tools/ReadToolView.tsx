// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
import {
  basename,
  extractPath,
  extractRange,
  extractTextContent,
} from '../../../utils/toolFormatters'

const UNCHANGED_RESULT_RE = /^\s*File unchanged since last read\.?\s*$/i

function isUnchangedOrEmptyResult(result: ToolViewProps['result']): boolean {
  if (!result) return false
  const text = extractTextContent(result.content).trim()
  if (!text) return true
  return UNCHANGED_RESULT_RE.test(text)
}

export function ReadHeader({ input, result }: ToolViewProps) {
  const path = extractPath(input)
  const range = extractRange(input)
  const tail = basename(path) || path || 'file'
  const lineSpan =
    range.offset !== undefined && range.limit !== undefined
      ? `L${range.offset}-${range.offset + range.limit - 1}`
      : range.offset !== undefined
        ? `L${range.offset}+`
        : ''
  const showUnchangedChip =
    !!result && !result.isError && isUnchangedOrEmptyResult(result)

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      <span className="min-w-0 flex-1 truncate">
        <span className="font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {tail}
        </span>
        {lineSpan && (
          <span className="ml-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
            {lineSpan}
          </span>
        )}
        {path && tail !== path && (
          <span
            className="ml-2 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
            title={path}
          >
            {path}
          </span>
        )}
      </span>
      {showUnchangedChip && (
        <span
          className="shrink-0 rounded-full border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-2 py-0.5 text-[10px] text-[var(--color-text-tertiary)]"
          title="File unchanged since last read"
        >
          unchanged
        </span>
      )}
    </span>
  )
}

export function ReadDetail({ input, result }: ToolViewProps) {
  const path = extractPath(input)
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const resultIsUnchanged = isUnchangedOrEmptyResult(result)

  return (
    <div className="space-y-2">
      {path && (
        <div className="flex items-center justify-between rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5">
          <span
            className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]"
            title={path}
          >
            {path}
          </span>
          <CopyButton
            text={path}
            className="ml-2 rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          />
        </div>
      )}
      <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
          <span>Tool Input</span>
          <CopyButton
            text={inputJson}
            className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          />
        </div>
        <CodeViewer code={inputJson} language="json" maxLines={10} />
      </div>
      {text && !resultIsUnchanged && (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <CodeViewer code={text} language="plaintext" maxLines={18} />
        </div>
      )}
    </div>
  )
}
