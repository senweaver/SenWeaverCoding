// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { CopyButton } from '../../shared/CopyButton'
import { useTranslation } from '../../../i18n'
import { extractTextContent } from '../../../utils/toolFormatters'
import { parseDocConvertEnvelope, stripDocConvertEnvelope } from '../utils/docConvertEnvelope'
import { revealInExplorer } from '../../../lib/revealInExplorer'
import { isTauriRuntime } from '../../../lib/desktopRuntime'

const FORMAT_STYLES: Record<string, string> = {
  pdf: 'bg-[#e5484d]/12 text-[#e5484d]',
  docx: 'bg-[#3b82f6]/12 text-[#3b82f6]',
  xlsx: 'bg-[#30a46c]/12 text-[#30a46c]',
  csv: 'bg-[#30a46c]/12 text-[#30a46c]',
  html: 'bg-[#f76b15]/12 text-[#f76b15]',
  md: 'bg-[var(--color-secondary)]/12 text-[var(--color-secondary)]',
}

function formatStyle(format: string): string {
  return (
    FORMAT_STYLES[format] ??
    'bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)]'
  )
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return ''
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

function fileName(path: string): string {
  const normalized = path.replace(/\\/g, '/')
  const parts = normalized.split('/').filter(Boolean)
  return parts[parts.length - 1] ?? path
}

function extOf(path: string): string {
  const name = fileName(path)
  const dot = name.lastIndexOf('.')
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : ''
}

function readInputString(input: unknown, key: string): string {
  if (!input || typeof input !== 'object') return ''
  const v = (input as Record<string, unknown>)[key]
  return typeof v === 'string' ? v.trim() : ''
}

function FormatBadge({ format }: { format: string }) {
  if (!format) return null
  return (
    <span
      className={`shrink-0 rounded-full px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] font-semibold uppercase ${formatStyle(format)}`}
    >
      {format}
    </span>
  )
}

export function DocumentHeader({ input, result }: ToolViewProps) {
  const text = result ? extractTextContent(result.content) : ''
  const envelope = text ? parseDocConvertEnvelope(text) : null

  const outputPath = envelope?.path ?? readInputString(input, 'output_path')
  const targetFormat =
    envelope?.format ?? readInputString(input, 'target_format').toLowerCase()
  const sourcePath = envelope?.source ?? readInputString(input, 'source_path')
  const sourceFormat = sourcePath ? extOf(sourcePath) : ''
  const size = envelope ? formatBytes(envelope.bytes) : ''

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      {sourceFormat && sourceFormat !== targetFormat && (
        <>
          <FormatBadge format={sourceFormat} />
          <span className="material-symbols-outlined shrink-0 text-[12px] text-[var(--color-text-tertiary)]">
            arrow_forward
          </span>
        </>
      )}
      <FormatBadge format={targetFormat} />
      <span
        className="min-w-0 truncate font-[var(--font-mono)] text-[12px]"
        title={outputPath}
      >
        {outputPath ? fileName(outputPath) : ''}
      </span>
      {size && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
          {size}
        </span>
      )}
    </span>
  )
}

export function DocumentDetail({ input, result }: ToolViewProps) {
  const t = useTranslation()
  const text = result ? extractTextContent(result.content) : ''
  const envelope = text ? parseDocConvertEnvelope(text) : null
  const summary = stripDocConvertEnvelope(text)

  const outputPath = envelope?.path ?? readInputString(input, 'output_path')
  const sourcePath = envelope?.source ?? readInputString(input, 'source_path')

  if (result?.isError) {
    return (
      <div className="whitespace-pre-wrap break-words font-[var(--font-mono)] text-[12px] text-[var(--color-error)]">
        {text || t('tool.error')}
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-2 text-[12px]">
      {sourcePath && (
        <div className="flex items-baseline gap-2">
          <span className="shrink-0 text-[11px] text-[var(--color-text-tertiary)]">
            {t('tool.document.source')}
          </span>
          <span
            className="min-w-0 truncate font-[var(--font-mono)] text-[var(--color-text-secondary)]"
            title={sourcePath}
          >
            {sourcePath}
          </span>
        </div>
      )}
      {outputPath && (
        <div className="flex items-baseline gap-2">
          <span className="shrink-0 text-[11px] text-[var(--color-text-tertiary)]">
            {t('tool.document.output')}
          </span>
          <span
            className="min-w-0 truncate font-[var(--font-mono)] text-[var(--color-text-secondary)]"
            title={outputPath}
          >
            {outputPath}
          </span>
        </div>
      )}
      {envelope?.font && (
        <div className="flex items-baseline gap-2">
          <span className="shrink-0 text-[11px] text-[var(--color-text-tertiary)]">
            {t('tool.document.font')}
          </span>
          <span className="min-w-0 truncate font-[var(--font-mono)] text-[var(--color-text-secondary)]">
            {envelope.font}
          </span>
        </div>
      )}
      {outputPath && (
        <div className="flex items-center gap-2 pt-1">
          {isTauriRuntime() && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                void revealInExplorer(outputPath).catch(() => {})
              }}
              className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
            >
              <span className="material-symbols-outlined text-[13px]">folder_open</span>
              {t('tool.document.reveal')}
            </button>
          )}
          <CopyButton
            text={outputPath}
            label={t('tool.document.copyPath')}
            copiedLabel={t('tool.document.copied')}
            className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
            onClick={(e) => e.stopPropagation()}
          />
        </div>
      )}
      {summary && (
        <div className="whitespace-pre-wrap break-words border-t border-[var(--color-border)]/40 pt-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {summary}
        </div>
      )}
    </div>
  )
}
