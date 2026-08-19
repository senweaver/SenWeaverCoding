// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type * as MonacoNs from 'monaco-editor'
import { useTranslation } from '../../i18n'
import { monaco } from '../../lib/monacoSetup'
import { formatBytes } from '../../lib/formatBytes'
import { formatAbsoluteTime } from '../../lib/formatRelativeTime'
import { ApiError } from '../../api/client'
import { fileHistoryApi, type FileHistoryEntry } from '../../api/fileHistory'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import { useFileHistoryStore } from '../../stores/fileHistoryStore'
import { useUIStore } from '../../stores/uiStore'
import { Modal } from '../shared/Modal'
import { Button } from '../shared/Button'
import { languageIdFor } from './MonacoFileEditor'

type Props = {
  root: string
  relPath: string
  name: string
  onClose: () => void
}

type SnapshotView = {
  content: string
  absent: boolean
  binary: boolean
  tooLarge: boolean
}

type CurrentView = {
  content: string
  state: 'ok' | 'missing' | 'binary' | 'unavailable'
}

export function FileHistoryPanel({ root, relPath, name, onClose }: Props) {
  const t = useTranslation()
  const theme = useUIStore((s) => s.theme)
  const addToast = useUIStore((s) => s.addToast)

  const [entries, setEntries] = useState<FileHistoryEntry[] | null>(null)
  const [listError, setListError] = useState<string | null>(null)
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null)
  const [snapshot, setSnapshot] = useState<SnapshotView | null>(null)
  const [snapshotLoading, setSnapshotLoading] = useState(false)
  const [current, setCurrent] = useState<CurrentView | null>(null)
  const [reverting, setReverting] = useState(false)
  const [confirming, setConfirming] = useState(false)

  const languageId = useMemo(() => languageIdFor(name), [name])

  const loadEntries = useCallback(async () => {
    setListError(null)
    try {
      const res = await fileHistoryApi.list({ root, path: relPath })
      setEntries(res.entries)
      setSelectedIndex((prev) => {
        if (
          prev !== null &&
          res.entries.some((entry) => entry.index === prev)
        ) {
          return prev
        }
        const last = res.entries[res.entries.length - 1]
        return last ? last.index : null
      })
    } catch (err) {
      setEntries([])
      setListError(err instanceof Error ? err.message : String(err))
    }
  }, [root, relPath])

  const loadCurrent = useCallback(async () => {
    try {
      const res = await workspaceFilesApi.readFile({ root, path: relPath })
      if (res.isBinary || res.encoding === 'base64') {
        setCurrent({ content: '', state: 'binary' })
      } else {
        setCurrent({ content: res.content ?? '', state: 'ok' })
      }
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) {
        setCurrent({ content: '', state: 'missing' })
      } else {
        setCurrent({ content: '', state: 'unavailable' })
      }
    }
  }, [root, relPath])

  useEffect(() => {
    void loadEntries()
    void loadCurrent()
  }, [loadEntries, loadCurrent])

  useEffect(() => {
    setConfirming(false)
    if (selectedIndex === null) {
      setSnapshot(null)
      return
    }
    let cancelled = false
    setSnapshotLoading(true)
    setSnapshot(null)
    fileHistoryApi
      .snapshot({ root, path: relPath, index: selectedIndex })
      .then((res) => {
        if (cancelled) return
        setSnapshot({
          content: res.content,
          absent: res.absent,
          binary: res.binary,
          tooLarge: res.tooLarge,
        })
      })
      .catch((err) => {
        if (cancelled) return
        setSnapshot(null)
        addToast({
          type: 'error',
          message: t('files.history.loadFailed', {
            message: err instanceof Error ? err.message : String(err),
          }),
        })
      })
      .finally(() => {
        if (!cancelled) setSnapshotLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [addToast, relPath, root, selectedIndex, t])

  const sortedEntries = useMemo(() => {
    if (!entries) return []
    return [...entries].sort((a, b) => b.index - a.index)
  }, [entries])

  const selectedEntry = useMemo(
    () =>
      selectedIndex === null
        ? undefined
        : entries?.find((entry) => entry.index === selectedIndex),
    [entries, selectedIndex],
  )

  const canDiff =
    snapshot !== null &&
    !snapshot.binary &&
    !snapshot.absent &&
    !snapshot.tooLarge &&
    current !== null &&
    current.state !== 'binary' &&
    current.state !== 'unavailable'

  const diffContainerRef = useRef<HTMLDivElement | null>(null)
  const diffEditorRef = useRef<MonacoNs.editor.IStandaloneDiffEditor | null>(null)

  useEffect(() => {
    const host = diffContainerRef.current
    if (!host || !canDiff || snapshot === null || current === null) return

    const initialTheme = useUIStore.getState().theme
    const original = monaco.editor.createModel(snapshot.content, languageId)
    const modified = monaco.editor.createModel(current.content, languageId)
    const diffEditor = monaco.editor.createDiffEditor(host, {
      automaticLayout: true,
      readOnly: true,
      originalEditable: false,
      renderSideBySide: true,
      fontSize: 12,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      renderOverviewRuler: false,
      theme: initialTheme === 'dark' ? 'vs-dark' : 'vs',
    })
    diffEditor.setModel({ original, modified })
    diffEditorRef.current = diffEditor

    return () => {
      diffEditor.dispose()
      original.dispose()
      modified.dispose()
      diffEditorRef.current = null
    }
  }, [canDiff, snapshot, current, languageId])

  useEffect(() => {
    monaco.editor.setTheme(theme === 'dark' ? 'vs-dark' : 'vs')
  }, [theme])

  const handleRevert = useCallback(async () => {
    if (selectedIndex === null || reverting) return
    setReverting(true)
    try {
      await fileHistoryApi.revert({
        root,
        path: relPath,
        index: selectedIndex,
        expectedSha256: selectedEntry?.sha256,
      })
      addToast({
        type: 'success',
        message: t('files.history.revertDone', { name }),
      })
      setConfirming(false)
      useFileHistoryStore.getState().scheduleRefresh(root)
      await Promise.all([loadEntries(), loadCurrent()])
    } catch (err) {
      addToast({
        type: 'error',
        message: t('files.history.revertFailed', {
          message: err instanceof Error ? err.message : String(err),
        }),
      })
    } finally {
      setReverting(false)
    }
  }, [
    addToast,
    loadCurrent,
    loadEntries,
    name,
    relPath,
    reverting,
    root,
    selectedEntry,
    selectedIndex,
    t,
  ])

  const sessionLabel = useCallback(
    (entry: FileHistoryEntry) => {
      if (entry.sessionName) return entry.sessionName
      if (entry.sessionId) return entry.sessionId
      return t('files.history.manualEdit')
    },
    [t],
  )

  const renderDiffArea = () => {
    if (selectedIndex === null) {
      return (
        <PanelNotice icon="difference" text={t('files.history.diffPlaceholder')} />
      )
    }
    if (snapshotLoading || current === null) {
      return <PanelNotice icon="progress_activity" text={t('files.history.loading')} spin />
    }
    if (snapshot === null) {
      return (
        <PanelNotice icon="error" text={t('files.history.snapshotUnavailable')} />
      )
    }
    if (snapshot.absent) {
      return <PanelNotice icon="scan_delete" text={t('files.history.absentLabel')} />
    }
    if (snapshot.tooLarge) {
      return (
        <PanelNotice icon="unfold_more" text={t('files.history.tooLargePreview')} />
      )
    }
    if (snapshot.binary || current.state === 'binary') {
      return (
        <PanelNotice icon="raw_on" text={t('files.history.binaryPreview')} />
      )
    }
    if (current.state === 'unavailable') {
      return (
        <PanelNotice
          icon="visibility_off"
          text={t('files.history.currentUnavailable')}
        />
      )
    }
    return <div ref={diffContainerRef} className="min-h-0 flex-1" />
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={t('files.history.title', { name })}
      width={1040}
      footer={
        <>
          {confirming ? (
            <>
              <span className="mr-auto self-center text-xs text-[var(--color-text-secondary)]">
                {selectedEntry?.absent
                  ? t('files.history.revertConfirmAbsent')
                  : t('files.history.revertConfirm')}
              </span>
              <Button
                variant="secondary"
                size="md"
                onClick={() => setConfirming(false)}
                disabled={reverting}
              >
                {t('common.cancel')}
              </Button>
              <Button
                variant="danger"
                size="md"
                onClick={() => {
                  void handleRevert()
                }}
                disabled={reverting}
              >
                {reverting
                  ? t('files.history.reverting')
                  : t('files.history.confirm')}
              </Button>
            </>
          ) : (
            <>
              <Button variant="secondary" size="md" onClick={onClose}>
                {t('common.close')}
              </Button>
              <Button
                variant="primary"
                size="md"
                onClick={() => setConfirming(true)}
                disabled={selectedIndex === null || reverting}
              >
                {t('files.history.revert')}
              </Button>
            </>
          )}
        </>
      }
    >
      <div className="flex h-[62vh] min-h-0 gap-3">
        <div className="flex w-80 flex-shrink-0 flex-col overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface-container)]">
          <div className="flex h-7 flex-shrink-0 items-center border-b border-[var(--color-border)] px-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
            {t('files.history.listTitle', {
              count: sortedEntries.length,
            })}
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">
            {entries === null && (
              <div className="px-3 py-3 text-xs text-[var(--color-text-tertiary)]">
                {t('files.history.loading')}
              </div>
            )}
            {entries !== null && listError && (
              <div className="px-3 py-3 text-xs text-[var(--color-danger)]">
                {t('files.history.loadFailed', { message: listError })}
              </div>
            )}
            {entries !== null && !listError && sortedEntries.length === 0 && (
              <div className="px-3 py-3 text-xs italic text-[var(--color-text-tertiary)]">
                {t('files.history.empty')}
              </div>
            )}
            {sortedEntries.map((entry) => {
              const active = entry.index === selectedIndex
              return (
                <button
                  key={entry.index}
                  type="button"
                  onClick={() => setSelectedIndex(entry.index)}
                  className={`block w-full border-b border-[var(--color-border)]/50 px-3 py-2 text-left transition-colors ${
                    active
                      ? 'bg-[var(--color-accent)]/15'
                      : 'hover:bg-[var(--color-surface-hover)]'
                  }`}
                >
                  <div className="flex items-center gap-1.5">
                    <span className="material-symbols-outlined text-[13px] text-[var(--color-text-tertiary)]">
                      {entry.absent ? 'note_add' : 'history'}
                    </span>
                    <span className="text-xs font-medium tabular-nums text-[var(--color-text-primary)]">
                      {formatAbsoluteTime(entry.timestamp * 1000)}
                    </span>
                  </div>
                  <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-[var(--color-text-secondary)]">
                    <span
                      className="material-symbols-outlined text-[12px] text-[var(--color-text-tertiary)]"
                      aria-hidden="true"
                    >
                      forum
                    </span>
                    <span className="truncate" title={sessionLabel(entry)}>
                      {sessionLabel(entry)}
                    </span>
                  </div>
                  <div className="mt-0.5 flex items-center gap-2 text-[10px] text-[var(--color-text-tertiary)]">
                    <span className="truncate">{entry.toolName}</span>
                    {entry.absent ? (
                      <span className="flex-shrink-0 italic">
                        {t('files.history.absentShort')}
                      </span>
                    ) : (
                      <span className="flex-shrink-0 tabular-nums">
                        {formatBytes(entry.byteSize)}
                      </span>
                    )}
                  </div>
                </button>
              )
            })}
          </div>
        </div>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)]">
          <div className="flex h-7 flex-shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface-elevated)] px-2 text-[11px]">
            <span className="flex items-center gap-1 text-[var(--color-text-secondary)]">
              <span
                aria-hidden="true"
                className="material-symbols-outlined text-[13px] text-[var(--color-text-tertiary)]"
              >
                difference
              </span>
              {t('files.history.snapshotLabel')}
              {selectedEntry
                ? ` · ${formatAbsoluteTime(selectedEntry.timestamp * 1000)}`
                : ''}
            </span>
            <span className="text-[var(--color-text-tertiary)]">
              {current?.state === 'missing'
                ? t('files.history.currentMissing')
                : t('files.history.currentLabel')}
            </span>
          </div>
          {renderDiffArea()}
        </div>
      </div>
    </Modal>
  )
}

function PanelNotice({
  icon,
  text,
  spin,
}: {
  icon: string
  text: string
  spin?: boolean
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 px-4 text-center">
      <span
        aria-hidden="true"
        className={`material-symbols-outlined text-[28px] text-[var(--color-text-tertiary)]${
          spin ? ' animate-spin' : ''
        }`}
      >
        {icon}
      </span>
      <p className="text-xs text-[var(--color-text-tertiary)]">{text}</p>
    </div>
  )
}
