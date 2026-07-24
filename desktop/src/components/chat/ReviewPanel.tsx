// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useReviewPanelStore } from '../../stores/reviewPanelStore'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useUIStore } from '../../stores/uiStore'
import { useLanGroupStore } from '../../stores/lanGroupStore'
import { useLanShareStore } from '../../stores/lanShareStore'
import { useTranslation } from '../../i18n'
import { splitPathForDisplay } from '../../utils/toolFormatters'
import { DiffViewer } from './DiffViewer'
import type { EditReviewFile } from '../../api/sessions'

const STATUS_ICONS: Record<EditReviewFile['status'], string> = {
  created: 'add_circle',
  modified: 'edit',
  deleted: 'delete',
}

const STATUS_COLORS: Record<EditReviewFile['status'], string> = {
  created: 'text-[var(--color-success)]',
  modified: 'text-[var(--color-warning)]',
  deleted: 'text-[var(--color-error)]',
}

export function ReviewPanel() {
  const t = useTranslation()
  const view = useReviewPanelStore(
    useShallow((s) => ({
      open: s.open,
      sessionId: s.sessionId,
      loading: s.loading,
      error: s.error,
      files: s.files,
      keptPaths: s.keptPaths,
      expandedPaths: s.expandedPaths,
      revertingPaths: s.revertingPaths,
    })),
  )
  const closePanel = useReviewPanelStore((s) => s.closePanel)
  const refresh = useReviewPanelStore((s) => s.refresh)
  const toggleExpanded = useReviewPanelStore((s) => s.toggleExpanded)
  const keepFile = useReviewPanelStore((s) => s.keepFile)
  const keepAll = useReviewPanelStore((s) => s.keepAll)
  const undoFile = useReviewPanelStore((s) => s.undoFile)
  const undoAll = useReviewPanelStore((s) => s.undoAll)
  const clearPendingEdits = useChatStore((s) => s.clearPendingEdits)
  const keepPendingEditFile = useChatStore((s) => s.keepPendingEditFile)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionRunning = useChatStore((s) =>
    view.sessionId
      ? (s.sessions[view.sessionId]?.chatState ?? 'idle') !== 'idle'
      : false,
  )
  const settingsOverlayOpen = useUIStore((s) => s.settingsOverlayOpen)
  const templateLibraryOpen = useUIStore((s) => s.templateLibraryOpen)
  const lanGroupPanelOpen = useLanGroupStore((s) => s.panelOpen)
  const lanSharePanelOpen = useLanShareStore((s) => s.panelOpen)
  const [undoErrorPath, setUndoErrorPath] = useState<string | null>(null)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return
      if (e.key === 'Escape') closePanel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [closePanel])

  useEffect(() => {
    if (view.open && view.sessionId && activeTabId && activeTabId !== view.sessionId) {
      closePanel()
    }
  }, [activeTabId, closePanel, view.open, view.sessionId])

  const siblingOverlayOpen =
    settingsOverlayOpen || templateLibraryOpen || lanGroupPanelOpen || lanSharePanelOpen
  useEffect(() => {
    if (view.open && siblingOverlayOpen) {
      closePanel()
    }
  }, [view.open, siblingOverlayOpen, closePanel])

  const visibleFiles = useMemo(
    () => view.files.filter((f) => !view.keptPaths[f.path]),
    [view.files, view.keptPaths],
  )

  const totals = useMemo(() => {
    let additions = 0
    let deletions = 0
    for (const f of visibleFiles) {
      additions += f.additions ?? 0
      deletions += f.deletions ?? 0
    }
    return { additions, deletions }
  }, [visibleFiles])

  if (!view.open || !view.sessionId) return null
  const sessionId = view.sessionId

  const onKeepAll = () => {
    if (sessionRunning) return
    keepAll()
    clearPendingEdits(sessionId)
  }

  const onUndoAll = async () => {
    if (sessionRunning) return
    await undoAll()
    clearPendingEdits(sessionId)
  }

  const onUndoFile = async (path: string) => {
    if (sessionRunning) return
    setUndoErrorPath(null)
    try {
      await undoFile(path)
      keepPendingEditFile(sessionId, path)
    } catch {
      setUndoErrorPath(path)
    }
  }

  const onKeepFile = (path: string) => {
    if (sessionRunning) return
    keepFile(path)
    keepPendingEditFile(sessionId, path)
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-3">
        <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)]">
          rate_review
        </span>
        <span className="text-[13px] font-semibold text-[var(--color-text-primary)]">
          {t('review.panelTitle')}
        </span>
        <span className="text-[11px] text-[var(--color-text-tertiary)]">
          {t('review.filesCount', { count: visibleFiles.length })}
        </span>
        {totals.additions > 0 && (
          <span className="rounded-full bg-[var(--color-success-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-success)]">
            +{totals.additions}
          </span>
        )}
        {totals.deletions > 0 && (
          <span className="rounded-full bg-[var(--color-error-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-error)]">
            -{totals.deletions}
          </span>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => void refresh()}
            disabled={view.loading}
            title={t('review.refresh')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)] disabled:opacity-40"
          >
            <span
              className={`material-symbols-outlined text-[15px]${view.loading ? ' animate-spin' : ''}`}
            >
              refresh
            </span>
          </button>
          {sessionRunning && (
            <span className="rounded-md bg-[var(--color-surface-container-high)] px-2 py-0.5 text-[10px] text-[var(--color-text-tertiary)]">
              {t('review.runningHint')}
            </span>
          )}
          {visibleFiles.length > 0 && !sessionRunning && (
            <>
              <button
                type="button"
                onClick={() => void onUndoAll()}
                disabled={view.loading}
                className="rounded-md border border-[var(--color-border)]/60 px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-container-high)] disabled:opacity-40"
              >
                {t('review.undoAll')}
              </button>
              <button
                type="button"
                onClick={onKeepAll}
                className="rounded-md bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-medium text-white transition-opacity hover:opacity-90"
              >
                {t('review.keepAll')}
              </button>
            </>
          )}
          <button
            type="button"
            onClick={closePanel}
            title={t('review.close')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[16px]">close</span>
          </button>
        </div>
      </div>

      {view.error && (
        <div className="border-b border-[var(--color-error)]/40 bg-[var(--color-error-container)]/40 px-3 py-1.5 text-[11px] text-[var(--color-error)]">
          {view.error}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {view.loading && visibleFiles.length === 0 && (
          <div className="px-4 py-6 text-center text-[12px] text-[var(--color-text-tertiary)]">
            {t('review.loading')}
          </div>
        )}
        {!view.loading && visibleFiles.length === 0 && (
          <div className="px-4 py-6 text-center text-[12px] text-[var(--color-text-tertiary)]">
            {t('review.empty')}
          </div>
        )}
        {visibleFiles.map((file) => (
          <ReviewFileRow
            key={file.path}
            file={file}
            expanded={Boolean(view.expandedPaths[file.path])}
            reverting={Boolean(view.revertingPaths[file.path])}
            undoError={undoErrorPath === file.path}
            readOnly={sessionRunning}
            onToggle={() => toggleExpanded(file.path)}
            onKeep={() => onKeepFile(file.path)}
            onUndo={() => void onUndoFile(file.path)}
          />
        ))}
      </div>
    </div>
  )
}

function ReviewFileRow({
  file,
  expanded,
  reverting,
  undoError,
  readOnly,
  onToggle,
  onKeep,
  onUndo,
}: {
  file: EditReviewFile
  expanded: boolean
  reverting: boolean
  undoError: boolean
  readOnly: boolean
  onToggle: () => void
  onKeep: () => void
  onUndo: () => void
}) {
  const t = useTranslation()
  const diff = useReviewPanelStore((s) => s.diffs[file.path])
  const diffLoading = useReviewPanelStore((s) =>
    Boolean(s.diffLoading[file.path]),
  )
  const loadDiff = useReviewPanelStore((s) => s.loadDiff)
  const retryDiff = useReviewPanelStore((s) => s.retryDiff)

  useEffect(() => {
    if (expanded && diff === undefined && !diffLoading) {
      void loadDiff(file.path)
    }
  }, [expanded, diff, diffLoading, loadDiff, file.path])

  const { dir, tail, separator } = splitPathForDisplay(file.path)

  return (
    <div className="border-b border-[var(--color-border)]/30">
      <div
        className="flex cursor-pointer items-center gap-2 px-3 py-2 transition-colors hover:bg-[var(--color-surface-container-low)]"
        onClick={onToggle}
        title={file.path}
      >
        <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
          {expanded ? 'expand_less' : 'expand_more'}
        </span>
        <span
          className={`material-symbols-outlined shrink-0 text-[14px] ${STATUS_COLORS[file.status]}`}
        >
          {STATUS_ICONS[file.status]}
        </span>
        {dir ? (
          <span className="min-w-0 flex flex-1 items-baseline truncate">
            <span className="min-w-0 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-tertiary)]">
              {dir}
              {separator}
            </span>
            <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
              {tail}
            </span>
          </span>
        ) : (
          <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
            {tail || file.path}
          </span>
        )}
        {typeof file.additions === 'number' && file.additions > 0 && (
          <span className="shrink-0 rounded-full bg-[var(--color-success-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-success)]">
            +{file.additions}
          </span>
        )}
        {typeof file.deletions === 'number' && file.deletions > 0 && (
          <span className="shrink-0 rounded-full bg-[var(--color-error-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-error)]">
            -{file.deletions}
          </span>
        )}
        {!readOnly && (
          <div className="flex shrink-0 items-center gap-1">
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                onUndo()
              }}
              disabled={reverting}
              title={t('review.undoFile')}
              className="inline-flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)] disabled:opacity-40"
            >
              <span
                className={`material-symbols-outlined text-[14px]${reverting ? ' animate-spin' : ''}`}
              >
                {reverting ? 'progress_activity' : 'undo'}
              </span>
            </button>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                onKeep()
              }}
              title={t('review.keepFile')}
              className="inline-flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-success)]"
            >
              <span className="material-symbols-outlined text-[14px]">check</span>
            </button>
          </div>
        )}
      </div>
      {undoError && (
        <div className="px-4 pb-1.5 text-[11px] text-[var(--color-error)]">
          {t('review.undoFileFailed')}
        </div>
      )}
      {expanded && (
        <div className="max-h-[480px] overflow-y-auto border-t border-[var(--color-border)]/20 bg-[var(--color-surface-container-lowest)]">
          {diffLoading && (
            <div className="px-4 py-3 text-center text-[11px] text-[var(--color-text-tertiary)]">
              {t('review.loading')}
            </div>
          )}
          {!diffLoading && diff != null && (
            <>
              {(diff.beforeTruncated || diff.afterTruncated) && (
                <div className="px-4 py-1 text-[10px] text-[var(--color-text-tertiary)]">
                  {t('review.diffTruncated')}
                </div>
              )}
              <DiffViewer
                filePath={file.path}
                oldString={diff.before}
                newString={diff.after}
              />
            </>
          )}
          {!diffLoading && diff === null && (
            <div className="flex items-center justify-center gap-2 px-4 py-3 text-[11px] text-[var(--color-text-tertiary)]">
              <span>{t('review.diffUnavailable')}</span>
              <button
                type="button"
                onClick={() => retryDiff(file.path)}
                className="rounded-md border border-[var(--color-border)]/60 px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-container-high)]"
              >
                {t('review.diffRetry')}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
