// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
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

function opsLabel(toolName: string): string {
  switch (toolName) {
    case 'backup':
      return 'Backup'
    case 'data_management':
      return 'Data'
    case 'security_ops':
      return 'Security'
    case 'cloud_ops':
      return 'Cloud'
    case 'cloud_patterns':
      return 'Cloud pattern'
    case 'canvas':
      return 'Canvas'
    case 'report_template':
      return 'Report template'
    default:
      return toolName
  }
}

export function OpsHeader({ toolName, input }: ToolViewProps) {
  const label = opsLabel(toolName)
  const action = readString(input, [
    'action',
    'op',
    'operation',
    'command',
    'method',
    'kind',
    'verb',
    'mode',
  ])
  const target = readString(input, [
    'target',
    'name',
    'template',
    'pattern',
    'path',
    'id',
    'key',
    'resource',
    'canvas_id',
  ])
  const count = readNumber(input, ['count', 'size', 'items_count'])

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-secondary)]">
        {label}
      </span>
      {action && (
        <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {truncate(action, 28)}
        </span>
      )}
      {target && (
        <span
          className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
          title={target}
        >
          · {truncate(target, 100)}
        </span>
      )}
      {count !== undefined && (
        <span className="shrink-0 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
          · {count}
        </span>
      )}
      {!action && !target && (
        <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {toolName}
        </span>
      )}
    </span>
  )
}

export function OpsDetail({ toolName, input, result }: ToolViewProps) {
  const label = opsLabel(toolName)
  const action = readString(input, [
    'action',
    'op',
    'operation',
    'command',
    'method',
    'kind',
    'verb',
    'mode',
  ])
  const target = readString(input, [
    'target',
    'name',
    'template',
    'pattern',
    'path',
    'id',
    'key',
    'resource',
    'canvas_id',
  ])
  const description = readString(input, ['description', 'summary', 'content'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)

  return (
    <div className="space-y-2">
      <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] space-y-1">
        <div className="flex items-center gap-2">
          <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            Ops
          </span>
          <span className="truncate text-[var(--color-text-secondary)]">
            {label} · {toolName}
          </span>
        </div>
        {action && (
          <div className="flex items-center gap-2 border-t border-[var(--color-border)]/40 pt-1">
            <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
              Action
            </span>
            <span className="truncate text-[var(--color-text-secondary)]">
              {action}
            </span>
          </div>
        )}
        {target && (
          <div className="flex items-center gap-2 border-t border-[var(--color-border)]/40 pt-1">
            <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
              Target
            </span>
            <span className="truncate text-[var(--color-text-secondary)]">
              {target}
            </span>
          </div>
        )}
        {description && (
          <div className="flex items-start gap-2 border-t border-[var(--color-border)]/40 pt-1">
            <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
              Details
            </span>
            <span className="whitespace-pre-wrap break-words text-[var(--color-text-secondary)]">
              {description}
            </span>
          </div>
        )}
      </div>
      {!action && !target && (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>Input</span>
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
          <CodeViewer code={text} language="plaintext" maxLines={16} />
        </div>
      )}
    </div>
  )
}
