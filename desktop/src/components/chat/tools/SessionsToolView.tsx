// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer'
import {
  extractTextContent,
  truncate,
} from '../../../utils/toolFormatters'

function readString(input: unknown, keys: string[]): string {
  if (!input || typeof input !== 'object') return ''
  const obj = input as Record<string, unknown>
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.trim()) return v
  }
  return ''
}

export function SessionsHeader({ toolName, input }: ToolViewProps) {
  const action = readString(input, ['action', 'op', 'method', 'kind'])
    || (toolName.startsWith('sessions_') ? toolName.slice('sessions_'.length) : '')
  const sessionId = readString(input, [
    'session_id',
    'sessionId',
    'id',
    'target_session',
  ])
  const parts: string[] = []
  if (action) parts.push(action)
  if (sessionId) parts.push(sessionId)
  const label = parts.length > 0 ? parts.join(' · ') : toolName
  return (
    <span
      className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
      title={label}
    >
      {truncate(label, 80)}
    </span>
  )
}

export function SessionsDetail({ input, result }: ToolViewProps) {
  const sessionId = readString(input, [
    'session_id',
    'sessionId',
    'id',
    'target_session',
  ])
  const title = readString(input, ['title', 'subject'])
  const content = readString(input, ['content', 'message', 'body'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)

  return (
    <div className="space-y-2">
      {(sessionId || title || content) && (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 text-[11px]">
          {sessionId && (
            <div className="flex items-center gap-2">
              <span className="text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                Session
              </span>
              <span className="truncate font-[var(--font-mono)] text-[var(--color-text-secondary)]">
                {sessionId}
              </span>
            </div>
          )}
          {title && (
            <div className="mt-1 flex items-center gap-2">
              <span className="text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                Title
              </span>
              <span className="truncate text-[var(--color-text-secondary)]">
                {title}
              </span>
            </div>
          )}
          {content && (
            <div className="mt-1 whitespace-pre-wrap break-words text-[var(--color-text-secondary)]">
              {content}
            </div>
          )}
        </div>
      )}
      {!sessionId && !title && !content && (
        <CodeViewer code={inputJson} language="json" maxLines={10} />
      )}
      {text && !result?.isError && (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2">
          <MarkdownRenderer content={text} />
        </div>
      )}
      {text && result?.isError && (
        <CodeViewer code={text} language="plaintext" maxLines={14} />
      )}
    </div>
  )
}
