// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
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

function readStringArray(input: unknown, keys: string[]): string[] {
  if (!input || typeof input !== 'object') return []
  const obj = input as Record<string, unknown>
  for (const k of keys) {
    const v = obj[k]
    if (Array.isArray(v)) {
      return v.filter((s): s is string => typeof s === 'string')
    }
  }
  return []
}

export function CommunicationHeader({ toolName, input }: ToolViewProps) {
  const recipient = readString(input, [
    'to',
    'recipient',
    'channel',
    'agent',
    'agent_id',
    'target',
    'target_agent',
  ])
    || (readStringArray(input, ['to', 'recipients']).join(', '))
  const body = readString(input, [
    'message',
    'text',
    'content',
    'question',
    'prompt',
    'body',
  ])
  return (
    <span
      className="min-w-0 flex-1 flex items-baseline gap-2 truncate text-[12px]"
      title={`${recipient ? `${recipient} — ` : ''}${body}`}
    >
      {recipient && (
        <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {truncate(recipient, 28)}
        </span>
      )}
      {body ? (
        <span className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {truncate(body.replace(/\s+/g, ' '), 100)}
        </span>
      ) : (
        !recipient && (
          <span className="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-tertiary)]">
            {toolName}
          </span>
        )
      )}
    </span>
  )
}

export function CommunicationDetail({ input, result }: ToolViewProps) {
  const recipient = readString(input, [
    'to',
    'recipient',
    'channel',
    'agent',
    'agent_id',
    'target',
    'target_agent',
  ])
    || (readStringArray(input, ['to', 'recipients']).join(', '))
  const body = readString(input, [
    'message',
    'text',
    'content',
    'question',
    'prompt',
    'body',
  ])
  const options = readStringArray(input, ['options', 'choices', 'answers'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)

  return (
    <div className="space-y-2">
      {(recipient || body || options.length > 0) ? (
        <div className="space-y-1.5">
          {recipient && (
            <div className="flex items-center gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1 text-[11px]">
              <span className="text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                To
              </span>
              <span className="truncate font-[var(--font-mono)] text-[var(--color-text-secondary)]">
                {recipient}
              </span>
            </div>
          )}
          {body && (
            <div className="whitespace-pre-wrap break-words rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-[12px] text-[var(--color-text-secondary)]">
              {body}
            </div>
          )}
          {options.length > 0 && (
            <ul className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] divide-y divide-[var(--color-border)]/40">
              {options.map((opt, idx) => (
                <li
                  key={`${opt}-${idx}`}
                  className="px-3 py-1 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]"
                >
                  {idx + 1}. {opt}
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <CodeViewer code={inputJson} language="json" maxLines={10} />
      )}
      {text && (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <CodeViewer code={text} language="plaintext" maxLines={14} />
        </div>
      )}
    </div>
  )
}
