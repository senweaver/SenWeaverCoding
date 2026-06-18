// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useLanStore } from '../../stores/lanStore'
import { useLanGroupStore } from '../../stores/lanGroupStore'
import type { LanGroupDocument, LanGroupSnapshot } from '../../types/lanGroup'
import { canContribute, canManage, formatBytes, isTauriRuntime, phaseLabel, pickPath } from './shared'

export function GroupDocuments({
  groupId,
  snapshot,
}: {
  groupId: string
  snapshot: LanGroupSnapshot
}) {
  const t = useTranslation()
  const selfId = useLanStore((s) => s.identity?.userId ?? '')
  const uploadDocument = useLanGroupStore((s) => s.uploadDocument)
  const downloadDocument = useLanGroupStore((s) => s.downloadDocument)
  const saveDocument = useLanGroupStore((s) => s.saveDocument)
  const removeDocument = useLanGroupStore((s) => s.removeDocument)

  const [phaseId, setPhaseId] = useState('')
  const [busy, setBusy] = useState(false)
  const editable = canContribute(snapshot.group.role)
  const manager = canManage(snapshot.group.role)

  const buckets = useMemo(() => {
    const known = new Set(snapshot.phases.map((p) => p.id))
    const groups: { id: string; label: string; docs: LanGroupDocument[] }[] = []
    for (const phase of snapshot.phases) {
      const docs = snapshot.documents.filter((d) => d.phaseId === phase.id)
      if (docs.length > 0) groups.push({ id: phase.id, label: phaseLabel(phase, t), docs })
    }
    const uncategorized = snapshot.documents.filter((d) => !known.has(d.phaseId))
    if (uncategorized.length > 0) {
      groups.push({ id: '', label: t('lanGroup.uncategorized'), docs: uncategorized })
    }
    return groups
  }, [snapshot, t])

  async function handleUpload(directory: boolean) {
    const path = await pickPath(directory)
    if (!path) return
    setBusy(true)
    try {
      await uploadDocument(groupId, path, phaseId, '')
    } finally {
      setBusy(false)
    }
  }

  async function handleSave(doc: LanGroupDocument) {
    let dest: string | null = null
    if (isTauriRuntime()) {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, multiple: false })
      dest = Array.isArray(selected) ? selected[0] : selected
    } else {
      dest = window.prompt(t('lan.saveDestPrompt'))
    }
    if (!dest) return
    await saveDocument(groupId, doc.id, dest)
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {editable && (
        <div className="flex items-center gap-2 border-b border-[var(--color-border)] p-2">
          <select
            value={phaseId}
            onChange={(e) => setPhaseId(e.target.value)}
            className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)] outline-none"
          >
            <option value="">{t('lanGroup.uncategorized')}</option>
            {snapshot.phases.map((p) => (
              <option key={p.id} value={p.id}>
                {phaseLabel(p, t)}
              </option>
            ))}
          </select>
          <button
            type="button"
            disabled={busy}
            onClick={() => void handleUpload(false)}
            className="inline-flex items-center gap-1 rounded-[var(--radius-md)] border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
          >
            <span className="material-symbols-outlined text-[14px]">attach_file</span>
            {t('lanGroup.uploadFile')}
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void handleUpload(true)}
            className="inline-flex items-center gap-1 rounded-[var(--radius-md)] border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
          >
            <span className="material-symbols-outlined text-[14px]">folder</span>
            {t('lanGroup.uploadFolder')}
          </button>
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-3">
        {buckets.length === 0 ? (
          <div className="py-6 text-center text-xs text-[var(--color-text-tertiary)]">
            {t('lanGroup.noDocuments')}
          </div>
        ) : (
          buckets.map((bucket) => (
            <div key={bucket.id || 'uncat'} className="mb-4">
              <div className="mb-1.5 text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
                {bucket.label}
              </div>
              <div className="space-y-2">
                {bucket.docs.map((doc) => {
                  const canDelete = manager || doc.uploader === selfId
                  return (
                    <div
                      key={doc.id}
                      className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-hover)] p-2.5"
                    >
                      <div className="flex items-start gap-2">
                        <span className="material-symbols-outlined text-[18px] text-[var(--color-text-tertiary)]">
                          {doc.isDir ? 'folder' : 'description'}
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                            {doc.name}
                          </div>
                          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[10px] text-[var(--color-text-tertiary)]">
                            <span>{formatBytes(doc.size)}</span>
                            <span>·</span>
                            <span>{doc.uploaderNickname}</span>
                            <span>·</span>
                            <span>
                              {t('lanGroup.version')}
                              {doc.version}
                            </span>
                            <span
                              className={`rounded px-1 ${
                                doc.available
                                  ? 'bg-[var(--color-success,#16a34a)] text-white'
                                  : 'bg-[var(--color-surface-selected)] text-[var(--color-text-secondary)]'
                              }`}
                            >
                              {doc.available
                                ? t('lanGroup.available')
                                : t('lanGroup.notDownloaded')}
                            </span>
                          </div>
                        </div>
                      </div>
                      <div className="mt-2 flex items-center gap-1.5">
                        {doc.available ? (
                          <button
                            type="button"
                            onClick={() => void handleSave(doc)}
                            className="inline-flex items-center gap-1 rounded-md bg-[var(--color-surface)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-brand)] hover:bg-[var(--color-surface-selected)]"
                          >
                            <span className="material-symbols-outlined text-[13px]">save</span>
                            {t('lanGroup.save')}
                          </button>
                        ) : (
                          <button
                            type="button"
                            onClick={() => void downloadDocument(groupId, doc.id)}
                            className="inline-flex items-center gap-1 rounded-md bg-[var(--color-surface)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-brand)] hover:bg-[var(--color-surface-selected)]"
                          >
                            <span className="material-symbols-outlined text-[13px]">download</span>
                            {t('lanGroup.download')}
                          </button>
                        )}
                        {canDelete && (
                          <button
                            type="button"
                            onClick={() => {
                              if (window.confirm(t('lanGroup.deleteDoc'))) {
                                void removeDocument(groupId, doc.id)
                              }
                            }}
                            className="inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-selected)] hover:text-[var(--color-error)]"
                          >
                            <span className="material-symbols-outlined text-[13px]">delete</span>
                          </button>
                        )}
                      </div>
                    </div>
                  )
                })}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
