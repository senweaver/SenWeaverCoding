// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { CodeViewer } from './CodeViewer'
import { useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { InlineImageGallery } from './InlineImageGallery'
import { isTauriRuntime } from '../../lib/desktopRuntime'

type Props = {
  content: unknown
  isError: boolean
  toolName?: string
  standalone?: boolean
}

async function openLocalPath(path: string): Promise<void> {
  if (!path) return
  if (!isTauriRuntime()) {
    window.open(path, '_blank')
    return
  }
  try {
    const mod = (await import(/* @vite-ignore */ '@tauri-apps/plugin-shell')) as {
      open: (target: string) => Promise<void>
    }
    await mod.open(path)
  } catch (err) {
    console.warn('[ToolResultBlock] open path failed', err)
  }
}

function extractQaDocPaths(
  content: unknown,
): { reportPath?: string; analysisPath?: string; runbookPath?: string } | null {
  const text = typeof content === 'string'
    ? content
    : Array.isArray(content)
      ? content
          .map((c: any) => (typeof c === 'string' ? c : c?.text || ''))
          .filter(Boolean)
          .join('\n')
      : ''
  if (!text) return null
  const start = text.indexOf('{')
  const end = text.lastIndexOf('}')
  if (start < 0 || end < 0 || end <= start) return null
  const candidate = text.slice(start, end + 1)
  let parsed: unknown
  try {
    parsed = JSON.parse(candidate)
  } catch {
    return null
  }
  if (!parsed || typeof parsed !== 'object') return null
  const obj = parsed as Record<string, unknown>
  const reportPath = typeof obj.report_path === 'string' ? obj.report_path : undefined
  const analysisPath =
    typeof obj.analysis_path === 'string' ? obj.analysis_path : undefined
  const runbookPath =
    typeof obj.runbook_path === 'string' ? obj.runbook_path : undefined
  if (!reportPath && !analysisPath && !runbookPath) return null
  return { reportPath, analysisPath, runbookPath }
}

const PREVIEW_LIMIT = 200

export function ToolResultBlock({ content, isError, toolName, standalone = true }: Props) {
  const [expanded, setExpanded] = useState(false)
  const t = useTranslation()

  const previewInfo = useMemo(() => extractTextPreview(content, PREVIEW_LIMIT + 1), [content])
  const fullText = useMemo(
    () => (expanded ? extractText(content) : ''),
    [content, expanded],
  )

  if (!standalone) return null

  const preview = previewInfo.text.slice(0, PREVIEW_LIMIT)
  const hasMore = previewInfo.truncated
  const text = expanded ? fullText : preview

  const qaDocs =
    !isError && toolName === 'debug_test_report'
      ? extractQaDocPaths(content)
      : null

  return (
    <div className={`mb-2 overflow-hidden rounded-xl border ${
      isError
        ? 'border-[var(--color-error)]/20'
        : 'border-[var(--color-outline-variant)]/20'
    }`}>
      {}
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        className={`flex w-full items-center justify-between px-3 py-2 text-left text-[10px] font-bold uppercase tracking-wider ${
        isError
          ? 'bg-[var(--color-error-container)] text-[var(--color-error)]'
          : 'bg-[var(--color-surface-container-high)] text-[var(--color-outline)]'
      }`}
      >
        <span className="flex items-center gap-1.5">
          <span className="material-symbols-outlined text-[12px]">
            {isError ? 'error' : 'check_circle'}
          </span>
          {toolName ? t('tool.result', { toolName }) : t('tool.resultGeneric')}
        </span>
        <span className={`px-2 py-0.5 rounded-full text-[9px] ${
          isError
            ? 'bg-[var(--color-error)]/10'
            : 'bg-[var(--color-diff-added-bg)] text-[var(--color-diff-added-text)]'
        }`}>
          {isError ? t('tool.error') : t('tool.success')}
        </span>
      </button>

      {}
      {expanded && <InlineImageGallery text={text} />}

      {}
      {expanded ? (
        isError ? (
          <div className="bg-[var(--color-error-container)]/50 px-3 py-2.5 font-[var(--font-mono)] text-[11px] leading-[1.5] whitespace-pre-wrap break-words text-[var(--color-error)]">
            {text}
          </div>
        ) : (
          <CodeViewer
            code={text}
            language="plaintext"
            maxLines={12}
          />
        )
      ) : (
        <div className="bg-[var(--color-surface-container-lowest)] px-3 py-2 font-[var(--font-mono)] text-[10px] leading-[1.35] text-[var(--color-text-tertiary)]">
          {preview}
          {hasMore ? '…' : ''}
        </div>
      )}

      {hasMore && (
        <button
          onClick={() => setExpanded((value) => !value)}
          className="w-full py-1 text-[10px] font-medium text-[var(--color-text-accent)] hover:underline bg-[var(--color-surface-container-low)] border-t border-[var(--color-outline-variant)]/10"
        >
          {expanded
            ? t('tool.showLess')
            : t('tool.showMore', {
                count: expanded ? Math.max(0, fullText.length - PREVIEW_LIMIT) : 0,
              })}
        </button>
      )}

      {qaDocs && (
        <div className="border-t border-[var(--color-outline-variant)]/20 bg-[var(--color-surface-container-low)] px-3 py-2.5">
          <div className="mb-1.5 flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-[var(--color-text-accent)]">
            <span className="material-symbols-outlined text-[12px]">
              description
            </span>
            {t('debug.qa.docs.title')}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {qaDocs.reportPath && (
              <button
                type="button"
                onClick={() => void openLocalPath(qaDocs.reportPath!)}
                className="inline-flex items-center gap-1 rounded-full border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-high)] px-2.5 py-1 text-[10px] font-medium text-[var(--color-text-primary)] hover:bg-[var(--color-primary-container)] hover:text-[var(--color-on-primary-container)] transition-colors"
                title={qaDocs.reportPath}
              >
                <span className="material-symbols-outlined text-[12px]">
                  assignment_turned_in
                </span>
                {t('debug.qa.docs.openReport')}
              </button>
            )}
            {qaDocs.analysisPath && (
              <button
                type="button"
                onClick={() => void openLocalPath(qaDocs.analysisPath!)}
                className="inline-flex items-center gap-1 rounded-full border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-high)] px-2.5 py-1 text-[10px] font-medium text-[var(--color-text-primary)] hover:bg-[var(--color-primary-container)] hover:text-[var(--color-on-primary-container)] transition-colors"
                title={qaDocs.analysisPath}
              >
                <span className="material-symbols-outlined text-[12px]">
                  insights
                </span>
                {t('debug.qa.docs.openAnalysis')}
              </button>
            )}
            {qaDocs.runbookPath && (
              <button
                type="button"
                onClick={() => void openLocalPath(qaDocs.runbookPath!)}
                className="inline-flex items-center gap-1 rounded-full border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-high)] px-2.5 py-1 text-[10px] font-medium text-[var(--color-text-primary)] hover:bg-[var(--color-primary-container)] hover:text-[var(--color-on-primary-container)] transition-colors"
                title={qaDocs.runbookPath}
              >
                <span className="material-symbols-outlined text-[12px]">
                  menu_book
                </span>
                {t('debug.qa.docs.openRunbook')}
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

function extractText(content: unknown): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map((c: any) => (typeof c === 'string' ? c : c?.text || ''))
      .filter(Boolean)
      .join('\n')
  }
  if (content && typeof content === 'object') {
    return JSON.stringify(content, null, 2)
  }
  return String(content ?? '')
}

function extractTextPreview(
  content: unknown,
  maxChars: number,
): { text: string; truncated: boolean } {
  if (typeof content === 'string') {
    if (content.length <= maxChars) return { text: content, truncated: false }
    return { text: content.slice(0, maxChars), truncated: true }
  }
  if (Array.isArray(content)) {
    let buf = ''
    for (const c of content as unknown[]) {
      const piece = typeof c === 'string' ? c : ((c as { text?: string })?.text || '')
      if (!piece) continue
      if (buf.length > 0) buf += '\n'
      buf += piece
      if (buf.length > maxChars) {
        return { text: buf.slice(0, maxChars), truncated: true }
      }
    }
    return { text: buf, truncated: buf.length > maxChars }
  }
  if (content && typeof content === 'object') {
    const full = JSON.stringify(content, null, 2)
    if (full.length <= maxChars) return { text: full, truncated: false }
    return { text: full.slice(0, maxChars), truncated: true }
  }
  const s = String(content ?? '')
  if (s.length <= maxChars) return { text: s, truncated: false }
  return { text: s.slice(0, maxChars), truncated: true }
}
