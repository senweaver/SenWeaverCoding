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

function serviceLabel(toolName: string): string {
  switch (toolName) {
    case 'linkedin':
      return 'LinkedIn'
    case 'notion':
      return 'Notion'
    case 'jira':
      return 'Jira'
    case 'microsoft365':
      return 'Microsoft 365'
    case 'whatsapp':
      return 'WhatsApp'
    default:
      return toolName
  }
}

export function IntegrationHeader({ toolName, input }: ToolViewProps) {
  const service = serviceLabel(toolName)
  const action = readString(input, [
    'action',
    'op',
    'method',
    'kind',
    'verb',
    'endpoint',
    'command',
  ])
  const target = readString(input, [
    'target',
    'resource',
    'path',
    'id',
    'name',
    'issue_key',
    'page_id',
    'database_id',
    'project',
    'to',
    'recipient',
    'query',
  ])

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-secondary)]">
        {service}
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
      {!action && !target && (
        <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {toolName}
        </span>
      )}
    </span>
  )
}

export function IntegrationDetail({ toolName, input, result }: ToolViewProps) {
  const service = serviceLabel(toolName)
  const action = readString(input, [
    'action',
    'op',
    'method',
    'kind',
    'verb',
    'endpoint',
    'command',
  ])
  const target = readString(input, [
    'target',
    'resource',
    'path',
    'id',
    'name',
    'issue_key',
    'page_id',
    'database_id',
    'project',
    'to',
    'recipient',
    'query',
  ])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const metaRows: Array<{ label: string; value: string }> = []
  metaRows.push({ label: 'Service', value: service })
  if (action) metaRows.push({ label: 'Action', value: action })
  if (target) metaRows.push({ label: 'Target', value: target })

  return (
    <div className="space-y-2">
      <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px]">
        {metaRows.map((row, idx) => (
          <div
            key={row.label}
            className={
              idx < metaRows.length - 1
                ? 'flex items-center gap-2 border-b border-[var(--color-border)]/40 pb-1 mb-1'
                : 'flex items-center gap-2'
            }
          >
            <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
              {row.label}
            </span>
            <span className="truncate text-[var(--color-text-secondary)]">
              {row.value}
            </span>
          </div>
        ))}
      </div>
      <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
          <span>Arguments</span>
          <CopyButton
            text={inputJson}
            className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          />
        </div>
        <CodeViewer code={inputJson} language="json" maxLines={10} />
      </div>
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
