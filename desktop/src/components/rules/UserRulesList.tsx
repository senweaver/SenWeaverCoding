// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { Button } from '../shared/Button'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useUserRulesStore } from '../../stores/userRulesStore'
import { useDockSuspend } from '../../hooks/useDockSuspend'
import { revealInExplorer } from '../../lib/revealInExplorer'
import type { UserRuleFile } from '../../api/userRules'
import { CreateRuleDialog } from './CreateRuleDialog'

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

export function UserRulesList() {
  const t = useTranslation()
  const directory = useUserRulesStore((s) => s.directory)
  const exists = useUserRulesStore((s) => s.exists)
  const files = useUserRulesStore((s) => s.files)
  const isLoading = useUserRulesStore((s) => s.isLoading)
  const error = useUserRulesStore((s) => s.error)
  const fetch = useUserRulesStore((s) => s.fetch)
  const loadContent = useUserRulesStore((s) => s.loadContent)
  const upsert = useUserRulesStore((s) => s.upsert)
  const remove = useUserRulesStore((s) => s.delete)
  const contentByPath = useUserRulesStore((s) => s.contentByPath)
  const loadingContentPaths = useUserRulesStore((s) => s.loadingContentPaths)
  const savingPaths = useUserRulesStore((s) => s.savingPaths)
  const addToast = useUIStore((s) => s.addToast)

  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [editing, setEditing] = useState<Record<string, string>>({})
  const [createOpen, setCreateOpen] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<UserRuleFile | null>(null)

  useEffect(() => {
    void fetch()
  }, [fetch])

  async function handleOpenDirectory() {
    if (!directory) return
    try {
      await revealInExplorer(directory)
      if (!exists) {
        addToast({ type: 'success', message: t('settings.userRules.createdHint') })
        void fetch()
      }
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.userRules.openFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }

  async function handleToggleExpand(file: UserRuleFile) {
    const next = new Set(expanded)
    if (next.has(file.path)) {
      next.delete(file.path)
      const editsCopy = { ...editing }
      delete editsCopy[file.path]
      setEditing(editsCopy)
    } else {
      next.add(file.path)
      if (contentByPath[file.path] === undefined) {
        await loadContent(file)
      }
    }
    setExpanded(next)
  }

  function handleStartEdit(file: UserRuleFile) {
    const current = contentByPath[file.path] ?? ''
    setEditing({ ...editing, [file.path]: current })
  }

  function handleCancelEdit(path: string) {
    const next = { ...editing }
    delete next[path]
    setEditing(next)
  }

  async function handleSaveEdit(file: UserRuleFile) {
    const draft = editing[file.path]
    if (draft === undefined) return
    try {
      await upsert(file.name, draft)
      addToast({ type: 'success', message: t('settings.userRules.savedToast') })
      handleCancelEdit(file.path)
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.userRules.saveFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }

  async function handleConfirmDelete() {
    if (!pendingDelete) return
    const target = pendingDelete
    setPendingDelete(null)
    try {
      await remove(target.name)
      addToast({ type: 'success', message: t('settings.userRules.deletedToast') })
      const next = new Set(expanded)
      next.delete(target.path)
      setExpanded(next)
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.userRules.deleteFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }

  return (
    <div className="space-y-4">
      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0 flex-1">
            <p className="text-xs font-semibold text-[var(--color-text-primary)] flex items-center gap-2">
              <span className="material-symbols-outlined text-[16px]">rule</span>
              {t('settings.userRules.title')}
            </p>
            <p className="text-xs text-[var(--color-text-tertiary)] mt-1 leading-relaxed">
              {t('settings.userRules.description')}
            </p>
            <div className="mt-2 flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
              <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
                folder
              </span>
              <code className="font-mono break-all text-xs px-1.5 py-0.5 rounded bg-[var(--color-surface-container-high)]">
                {directory ?? '—'}
              </code>
            </div>
          </div>
          <div className="flex flex-col gap-2 flex-shrink-0">
            <Button size="sm" onClick={() => setCreateOpen(true)} disabled={!directory}>
              <span className="material-symbols-outlined text-[14px] mr-1">add</span>
              {t('settings.userRules.newButton')}
            </Button>
            <button
              onClick={handleOpenDirectory}
              disabled={!directory}
              className="inline-flex items-center gap-1.5 h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)] whitespace-nowrap disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined text-[14px]">folder_open</span>
              {t('settings.userRules.openDirectory')}
            </button>
          </div>
        </div>
        <div className="rounded-md border border-dashed border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-xs text-[var(--color-text-secondary)] leading-relaxed">
          {t('settings.userRules.tierExplanation')}
        </div>
      </section>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      {isLoading && files.length === 0 ? (
        <div className="flex justify-center py-10">
          <div className="animate-spin w-5 h-5 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
        </div>
      ) : files.length === 0 ? (
        <div className="rounded-lg border border-dashed border-[var(--color-border)] p-4 text-center text-xs text-[var(--color-text-secondary)] space-y-1">
          <p>{t('settings.userRules.empty')}</p>
          <p className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.userRules.emptyHint')}
          </p>
        </div>
      ) : (
        <ul className="space-y-2">
          {files.map((file) => {
            const isExpanded = expanded.has(file.path)
            const isLoadingContent = loadingContentPaths.has(file.path)
            const isSaving = savingPaths.has(file.name)
            const editingDraft = editing[file.path]
            const isEditing = editingDraft !== undefined
            const cachedContent = contentByPath[file.path]

            return (
              <li
                key={file.path}
                className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] hover:border-[var(--color-border-focus)] transition-colors"
              >
                <div
                  onClick={() => void handleToggleExpand(file)}
                  className="cursor-pointer p-3 flex items-start justify-between gap-3"
                >
                  <div className="min-w-0 flex-1 space-y-1">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
                        description
                      </span>
                      <span className="text-xs font-semibold text-[var(--color-text-primary)] font-mono break-all">
                        {file.name}
                      </span>
                      <span className="text-[10px] text-[var(--color-text-tertiary)] flex-shrink-0">
                        {formatBytes(file.size)}
                      </span>
                      {file.alwaysApply ? (
                        <span
                          title={t('settings.userRules.alwaysApplyHint')}
                          className="rounded-full bg-[var(--color-success-container)] text-[var(--color-success)] px-2 py-0.5 text-[10px] font-medium"
                        >
                          {t('settings.userRules.alwaysApplyBadge')}
                        </span>
                      ) : (
                        <span
                          title={t('settings.userRules.onDemandHint')}
                          className="rounded-full bg-[var(--color-info-container)] text-[var(--color-info)] px-2 py-0.5 text-[10px] font-medium"
                        >
                          {t('settings.userRules.onDemandBadge')}
                        </span>
                      )}
                    </div>
                    {file.description && (
                      <p className="text-xs text-[var(--color-text-primary)] leading-relaxed line-clamp-1 break-words">
                        {file.description}
                      </p>
                    )}
                    {file.summary && !isExpanded && (
                      <p className="text-xs text-[var(--color-text-secondary)] leading-relaxed line-clamp-2 break-words">
                        {file.summary}
                      </p>
                    )}
                    <p className="text-[10px] text-[var(--color-text-tertiary)] font-mono truncate">
                      {file.path}
                    </p>
                  </div>
                  <span
                    className={`material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)] flex-shrink-0 transition-transform ${isExpanded ? 'rotate-90' : ''}`}
                  >
                    chevron_right
                  </span>
                </div>

                {isExpanded && (
                  <div className="border-t border-[var(--color-border)] p-3 space-y-2">
                    {isLoadingContent && cachedContent === undefined ? (
                      <div className="flex justify-center py-6">
                        <div className="animate-spin w-4 h-4 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
                      </div>
                    ) : isEditing ? (
                      <>
                        <textarea
                          value={editingDraft}
                          onChange={(e) =>
                            setEditing({ ...editing, [file.path]: e.target.value })
                          }
                          className="w-full min-h-[260px] max-h-[480px] font-mono text-xs leading-5 px-2.5 py-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-primary)] resize-y focus:outline-none focus:border-[var(--color-brand)]"
                          spellCheck={false}
                        />
                        <div className="flex items-center justify-between gap-2 flex-wrap">
                          <p className="text-xs text-[var(--color-text-tertiary)]">
                            {t('settings.userRules.editingHint')}
                          </p>
                          <div className="flex items-center gap-2">
                            <button
                              onClick={() => handleCancelEdit(file.path)}
                              disabled={isSaving}
                              className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)] disabled:opacity-50"
                            >
                              {t('common.cancel')}
                            </button>
                            <Button
                              size="sm"
                              onClick={() => void handleSaveEdit(file)}
                              disabled={isSaving}
                            >
                              <span className="material-symbols-outlined text-[14px] mr-1">
                                save
                              </span>
                              {isSaving ? t('common.saving') : t('common.save')}
                            </Button>
                          </div>
                        </div>
                      </>
                    ) : (
                      <>
                        <pre className="w-full max-h-[480px] overflow-auto font-mono text-xs leading-5 px-2.5 py-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] text-[var(--color-text-primary)] whitespace-pre-wrap break-words">
                          {cachedContent ?? ''}
                        </pre>
                        <div className="flex items-center justify-end gap-2">
                          <button
                            onClick={() => void handleConfirmDeleteOpen(file)}
                            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-error-container)] hover:text-[var(--color-error)] text-[var(--color-text-secondary)]"
                          >
                            <span className="material-symbols-outlined text-[14px]">
                              delete
                            </span>
                            {t('common.delete')}
                          </button>
                          <button
                            onClick={() => void handleRevealFile(file.path)}
                            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]"
                          >
                            <span className="material-symbols-outlined text-[14px]">
                              folder_open
                            </span>
                            {t('settings.userRules.revealFile')}
                          </button>
                          <Button size="sm" onClick={() => handleStartEdit(file)}>
                            <span className="material-symbols-outlined text-[14px] mr-1">
                              edit
                            </span>
                            {t('common.edit')}
                          </Button>
                        </div>
                      </>
                    )}
                  </div>
                )}
              </li>
            )
          })}
        </ul>
      )}

      <CreateRuleDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreated={() => {
          setCreateOpen(false)
          addToast({ type: 'success', message: t('settings.userRules.createdToast') })
          void fetch()
        }}
      />

      {pendingDelete && (
        <DeleteConfirmModal
          fileName={pendingDelete.name}
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => void handleConfirmDelete()}
        />
      )}
    </div>
  )

  function handleConfirmDeleteOpen(file: UserRuleFile) {
    setPendingDelete(file)
  }

  async function handleRevealFile(path: string) {
    try {
      await revealInExplorer(path)
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.userRules.openFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }
}

function DeleteConfirmModal({
  fileName,
  onCancel,
  onConfirm,
}: {
  fileName: string
  onCancel: () => void
  onConfirm: () => void
}) {
  const t = useTranslation()
  useDockSuspend(true)
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4">
      <div className="w-full max-w-md rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 space-y-3 shadow-xl">
        <div>
          <p className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t('settings.userRules.deleteConfirmTitle')}
          </p>
          <p className="mt-1 text-xs text-[var(--color-text-secondary)] break-all">
            {t('settings.userRules.deleteConfirmBody', { name: fileName })}
          </p>
        </div>
        <div className="flex items-center justify-end gap-2">
          <button
            onClick={onCancel}
            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]"
          >
            {t('common.cancel')}
          </button>
          <button
            onClick={onConfirm}
            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] bg-[var(--color-error)] text-white hover:opacity-90"
          >
            {t('common.delete')}
          </button>
        </div>
      </div>
    </div>
  )
}
