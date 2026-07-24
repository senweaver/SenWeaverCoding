// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, useState } from 'react'
import type * as MonacoNs from 'monaco-editor'
import { useTranslation } from '../../i18n'
import { monaco } from '../../lib/monacoSetup'
import { useUIStore } from '../../stores/uiStore'

type Props = {

  previousContent: string

  currentContent: string

  languageId: string

  onClose: () => void

  editable?: boolean

  onApply?: (content: string) => void
}

export function MonacoDiffOverlay({
  previousContent,
  currentContent,
  languageId,
  onClose,
  editable = false,
  onApply,
}: Props) {
  const t = useTranslation()
  const theme = useUIStore((s) => s.theme)
  const containerRef = useRef<HTMLDivElement | null>(null)
  const editorRef = useRef<MonacoNs.editor.IStandaloneDiffEditor | null>(null)
  const modelsRef = useRef<{
    original: MonacoNs.editor.ITextModel
    modified: MonacoNs.editor.ITextModel
  } | null>(null)
  const [dirty, setDirty] = useState(false)

  useEffect(() => {
    const host = containerRef.current
    if (!host) return

    const original = monaco.editor.createModel(previousContent, languageId)
    const modified = monaco.editor.createModel(currentContent, languageId)
    modelsRef.current = { original, modified }

    const diffEditor = monaco.editor.createDiffEditor(host, {
      automaticLayout: true,
      readOnly: !editable,
      originalEditable: false,
      renderSideBySide: true,
      renderMarginRevertIcon: editable,
      fontSize: 13,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      renderOverviewRuler: false,
      theme: theme === 'dark' ? 'vs-dark' : 'vs',
    })
    diffEditor.setModel({ original, modified })
    editorRef.current = diffEditor

    const changeSub = editable
      ? modified.onDidChangeContent(() => {
          setDirty(modified.getValue() !== currentContent)
        })
      : null

    return () => {
      changeSub?.dispose()
      diffEditor.dispose()
      original.dispose()
      modified.dispose()
      editorRef.current = null
      modelsRef.current = null
    }

    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previousContent, currentContent, languageId, editable])

  useEffect(() => {
    monaco.editor.setTheme(theme === 'dark' ? 'vs-dark' : 'vs')
  }, [theme])

  const handleRevertAll = () => {
    const models = modelsRef.current
    if (!models) return
    models.modified.setValue(previousContent)
  }

  const handleApply = () => {
    const models = modelsRef.current
    if (!models || !onApply) return
    onApply(models.modified.getValue())
  }

  return (
    <div className="absolute inset-0 z-20 flex flex-col bg-[var(--color-surface)]">
      <div className="flex h-7 flex-shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface-elevated)] px-2 text-xs">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
            difference
          </span>
          <span className="font-medium text-[var(--color-text-secondary)]">
            {t('files.diffMode.title')}
          </span>
          {editable && (
            <span className="truncate text-[10px] text-[var(--color-text-tertiary)]">
              {t('files.diffMode.perHunkHint')}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1">
          {editable && (
            <button
              type="button"
              onClick={handleRevertAll}
              className="rounded px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
            >
              {t('files.diffMode.revertAll')}
            </button>
          )}
          {editable && onApply && (
            <button
              type="button"
              onClick={handleApply}
              disabled={!dirty}
              className="rounded bg-[var(--color-accent)]/15 px-2 py-0.5 text-[11px] font-medium text-[var(--color-accent)] hover:bg-[var(--color-accent)]/25 disabled:opacity-40 disabled:hover:bg-[var(--color-accent)]/15"
            >
              {t('files.diffMode.apply')}
            </button>
          )}
          <button
            type="button"
            onClick={onClose}
            aria-label={t('files.diffMode.dismiss')}
            title={t('files.diffMode.dismiss')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[14px]">close</span>
          </button>
        </div>
      </div>
      <div ref={containerRef} className="min-h-0 flex-1" />
    </div>
  )
}
