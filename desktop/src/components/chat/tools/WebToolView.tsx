// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import {
  extractAction,
  extractQuery,
  extractTextContent,
  extractUrl,
  truncate,
  urlHost,
} from '../../../utils/toolFormatters'

export function WebHeader({ input, toolName }: ToolViewProps) {
  const url = extractUrl(input)
  const query = extractQuery(input)
  const action = extractAction(input)
  const host = url ? urlHost(url) : ''
  const label = host || url || query || action || toolName || 'web'
  const title = url || query || action || toolName
  return (
    <span
      className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
      title={title}
    >
      {truncate(label, 80)}
    </span>
  )
}

export function WebDetail({ input, result }: ToolViewProps) {
  const url = extractUrl(input)
  const query = extractQuery(input)
  const text = result ? extractTextContent(result.content) : ''

  return (
    <div className="space-y-2">
      {url && (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
          <a
            href={url}
            target="_blank"
            rel="noreferrer"
            className="hover:underline text-[var(--color-text-accent)]"
          >
            {url}
          </a>
        </div>
      )}
      {query && (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
          {query}
        </div>
      )}
      {text && (
        <CodeViewer code={text} language="plaintext" maxLines={20} />
      )}
    </div>
  )
}
