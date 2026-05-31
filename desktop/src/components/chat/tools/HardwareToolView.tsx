// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
import { TerminalChrome } from '../TerminalChrome'
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

function hwLabel(toolName: string): string {
  switch (toolName) {
    case 'device_read_code':
      return 'Read code'
    case 'device_write_code':
      return 'Write code'
    case 'device_exec':
      return 'Run'
    case 'gpio_read':
      return 'GPIO read'
    case 'gpio_write':
      return 'GPIO write'
    case 'pico_flash':
      return 'Flash'
    case 'hardware_memory_map':
      return 'Memory map'
    default:
      return toolName
  }
}

export function HardwareHeader({ toolName, input }: ToolViewProps) {
  const label = hwLabel(toolName)
  const device = readString(input, [
    'device',
    'device_id',
    'target',
    'board',
    'path',
    'port',
  ])
  const pin = readNumber(input, ['pin', 'gpio', 'address'])
  const value = readString(input, ['value', 'state'])

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {label}
      </span>
      {device && (
        <span
          className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
          title={device}
        >
          · {truncate(device, 60)}
        </span>
      )}
      {pin !== undefined && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-secondary)]">
          pin {pin}
        </span>
      )}
      {value && (
        <span className="shrink-0 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
          = {truncate(value, 16)}
        </span>
      )}
    </span>
  )
}

export function HardwareDetail({ toolName, input, result }: ToolViewProps) {
  const label = hwLabel(toolName)
  const device = readString(input, [
    'device',
    'device_id',
    'target',
    'board',
    'path',
    'port',
  ])
  const pin = readNumber(input, ['pin', 'gpio', 'address'])
  const value = readString(input, ['value', 'state'])
  const code = readString(input, ['code', 'program', 'source', 'script'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)

  return (
    <div className="space-y-2">
      <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] space-y-1">
        <div className="flex items-center gap-2">
          <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            Tool
          </span>
          <span className="truncate text-[var(--color-text-secondary)]">
            {label} · {toolName}
          </span>
        </div>
        {device && (
          <div className="flex items-center gap-2 border-t border-[var(--color-border)]/40 pt-1">
            <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
              Device
            </span>
            <span className="truncate text-[var(--color-text-secondary)]">
              {device}
            </span>
          </div>
        )}
        {pin !== undefined && (
          <div className="flex items-center gap-2 border-t border-[var(--color-border)]/40 pt-1">
            <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
              Pin
            </span>
            <span className="truncate text-[var(--color-text-secondary)]">
              {pin}
            </span>
          </div>
        )}
        {value && (
          <div className="flex items-center gap-2 border-t border-[var(--color-border)]/40 pt-1">
            <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
              Value
            </span>
            <span className="truncate text-[var(--color-text-secondary)]">
              {value}
            </span>
          </div>
        )}
      </div>
      {code ? (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>Source</span>
            <CopyButton
              text={code}
              className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <CodeViewer code={code} language="plaintext" maxLines={18} />
        </div>
      ) : (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <CodeViewer code={inputJson} language="json" maxLines={8} />
        </div>
      )}
      {text && (
        <TerminalChrome title={label}>
          <div className="px-3 py-2 font-[var(--font-mono)] text-[11px] leading-[1.45] text-[var(--color-terminal-fg)] whitespace-pre-wrap break-words">
            {text}
          </div>
        </TerminalChrome>
      )}
    </div>
  )
}
