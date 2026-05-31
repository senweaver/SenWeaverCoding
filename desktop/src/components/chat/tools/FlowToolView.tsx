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

function readNumber(input: unknown, keys: string[]): number | undefined {
  if (!input || typeof input !== 'object') return undefined
  const obj = input as Record<string, unknown>
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'number' && Number.isFinite(v)) return v
    if (typeof v === 'string') {
      const n = Number(v)
      if (Number.isFinite(n)) return n
    }
  }
  return undefined
}

export function FlowHeader({ toolName, input }: ToolViewProps) {
  const flow = readString(input, ['flow', 'pipeline', 'name', 'id'])
  const step = readString(input, ['step', 'stage', 'phase', 'action'])
  const seconds = readNumber(input, ['seconds', 'duration', 'delay_ms'])
  const parts: string[] = []
  if (flow) parts.push(flow)
  if (step && step !== flow) parts.push(step)
  if (toolName === 'sleep' && seconds !== undefined) {
    parts.push(`${seconds}s`)
  }
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

export function FlowDetail({ input, result }: ToolViewProps) {
  const flow = readString(input, ['flow', 'pipeline', 'name', 'id'])
  const step = readString(input, ['step', 'stage', 'phase', 'action'])
  const description = readString(input, ['description', 'summary'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const metaRows: Array<{ label: string; value: string }> = []
  if (flow) metaRows.push({ label: 'Flow', value: flow })
  if (step && step !== flow) metaRows.push({ label: 'Step', value: step })
  if (description) metaRows.push({ label: 'Description', value: description })

  return (
    <div className="space-y-2">
      {metaRows.length > 0 ? (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px]">
          {metaRows.map((r, idx) => (
            <div
              key={r.label}
              className={
                idx < metaRows.length - 1
                  ? 'flex items-center gap-2 border-b border-[var(--color-border)]/40 pb-1 mb-1'
                  : 'flex items-center gap-2'
              }
            >
              <span className="w-20 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                {r.label}
              </span>
              <span className="truncate text-[var(--color-text-secondary)]">
                {r.value}
              </span>
            </div>
          ))}
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
