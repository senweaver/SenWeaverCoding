// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
import {
  basename,
  extractPath,
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

function readSymbolLabel(input: unknown): string {
  return readString(input, [
    'symbol',
    'symbol_name',
    'name',
    'query',
    'selector',
    'target',
    'spec',
    'signature',
  ])
}

export function CodeIntelHeader({ toolName, input }: ToolViewProps) {
  const rawPath = extractPath(input)
  const symbol = readSymbolLabel(input)
  const action = readString(input, ['action', 'method', 'kind', 'mode'])
  const parts: string[] = []
  if (action) parts.push(action)
  if (symbol) parts.push(symbol)
  const label = parts.length > 0 ? parts.join(' · ') : toolName
  const file = rawPath ? basename(rawPath) : ''

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 truncate text-[12px] text-[var(--color-text-secondary)]">
      <span
        className="min-w-0 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
        title={rawPath ? `${rawPath} · ${label}` : label}
      >
        {truncate(label, 80)}
      </span>
      {file && (
        <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {file}
        </span>
      )}
    </span>
  )
}

export function CodeIntelDetail({ toolName, input, result }: ToolViewProps) {
  const path = extractPath(input)
  const symbol = readSymbolLabel(input)
  const action = readString(input, ['action', 'method', 'kind', 'mode'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const metaRows: Array<{ label: string; value: string }> = []
  if (action) metaRows.push({ label: 'Action', value: action })
  if (symbol) metaRows.push({ label: 'Symbol', value: symbol })
  if (path) metaRows.push({ label: 'Path', value: path })

  return (
    <div className="space-y-2">
      {metaRows.length > 0 && (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
          <table className="w-full text-[11px]">
            <tbody>
              {metaRows.map((row, idx) => (
                <tr
                  key={row.label}
                  className={
                    idx < metaRows.length - 1
                      ? 'border-b border-[var(--color-border)]/40'
                      : ''
                  }
                >
                  <td className="w-16 px-3 py-1 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                    {row.label}
                  </td>
                  <td className="truncate px-3 py-1 font-[var(--font-mono)] text-[var(--color-text-secondary)]">
                    {row.value}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {!metaRows.length && (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>Tool Input · {toolName}</span>
            <CopyButton
              text={inputJson}
              className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <CodeViewer code={inputJson} language="json" maxLines={10} />
        </div>
      )}
      {text && (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>{result?.isError ? 'Error' : 'Output'}</span>
            <CopyButton
              text={text}
              className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <CodeViewer code={text} language="plaintext" maxLines={20} />
        </div>
      )}
    </div>
  )
}
