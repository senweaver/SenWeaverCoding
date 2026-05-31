// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'
import { ConfirmDialog } from '../shared/ConfirmDialog'
import { useSubagentsStore } from '../../stores/subagentsStore'
import { useUIStore } from '../../stores/uiStore'
import { SubagentEditor } from './SubagentEditor'
import type { DelegateAgentDef } from '../../types/subagents'

type Mode =
  | { kind: 'list' }
  | { kind: 'create' }
  | { kind: 'edit'; agent: DelegateAgentDef }

export function SubagentList() {
  const t = useTranslation()
  const agents = useSubagentsStore((s) => s.agents)
  const isLoading = useSubagentsStore((s) => s.isLoading)
  const isSaving = useSubagentsStore((s) => s.isSaving)
  const error = useSubagentsStore((s) => s.error)
  const fetch = useSubagentsStore((s) => s.fetch)
  const create = useSubagentsStore((s) => s.create)
  const update = useSubagentsStore((s) => s.update)
  const remove = useSubagentsStore((s) => s.remove)
  const addToast = useUIStore((s) => s.addToast)
  const [mode, setMode] = useState<Mode>({ kind: 'list' })
  const [pendingDelete, setPendingDelete] = useState<DelegateAgentDef | null>(null)

  useEffect(() => {
    void fetch()
  }, [fetch])

  async function handleCreate(def: DelegateAgentDef) {
    await create(def)
    addToast({ type: 'success', message: t('settings.subagents.createdToast') })
    setMode({ kind: 'list' })
  }

  async function handleUpdate(def: DelegateAgentDef) {
    if (mode.kind !== 'edit') return
    const { name: _ignored, ...patch } = def
    await update(mode.agent.name, patch)
    addToast({ type: 'success', message: t('settings.subagents.updatedToast') })
    setMode({ kind: 'list' })
  }

  async function confirmDelete() {
    if (!pendingDelete) return
    try {
      await remove(pendingDelete.name)
      addToast({ type: 'success', message: t('settings.subagents.deletedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setPendingDelete(null)
    }
  }

  if (mode.kind !== 'list') {
    return (
      <SubagentEditor
        initial={mode.kind === 'edit' ? mode.agent : null}
        isSaving={isSaving}
        onSubmit={mode.kind === 'edit' ? handleUpdate : handleCreate}
        onCancel={() => setMode({ kind: 'list' })}
      />
    )
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-xs text-[var(--color-text-secondary)]">
          {t('settings.subagents.listDescription')}
        </p>
        <Button size="sm" onClick={() => setMode({ kind: 'create' })}>
          <span className="material-symbols-outlined text-[14px] mr-1">add</span>
          {t('settings.subagents.create')}
        </Button>
      </div>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      {agents.length === 0 && !isLoading ? (
        <div className="rounded-lg border border-dashed border-[var(--color-border)] p-4 text-center text-xs text-[var(--color-text-secondary)]">
          {t('settings.subagents.empty')}
        </div>
      ) : (
        <ul className="grid grid-cols-1 lg:grid-cols-2 gap-3">
          {agents.map((agent) => (
            <li
              key={agent.name}
              className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-2"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-xs font-semibold text-[var(--color-text-primary)] truncate">
                    {agent.name}
                  </p>
                  <p className="text-xs text-[var(--color-text-tertiary)] font-mono truncate">
                    {agent.provider}/{agent.model}
                  </p>
                </div>
                <div className="flex items-center gap-1 flex-shrink-0">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setMode({ kind: 'edit', agent })}
                  >
                    <span className="material-symbols-outlined text-[14px]">edit</span>
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setPendingDelete(agent)}
                  >
                    <span className="material-symbols-outlined text-[14px]">delete</span>
                  </Button>
                </div>
              </div>
              {agent.systemPrompt && (
                <p className="text-xs text-[var(--color-text-secondary)] line-clamp-3">
                  {agent.systemPrompt}
                </p>
              )}
              {agent.allowedTools.length > 0 && (
                <div className="flex flex-wrap gap-1">
                  {agent.allowedTools.slice(0, 8).map((tool) => (
                    <span
                      key={tool}
                      className="text-[10px] rounded bg-[var(--color-surface)] border border-[var(--color-border)] px-1.5 py-[1px] font-mono"
                    >
                      {tool}
                    </span>
                  ))}
                  {agent.allowedTools.length > 8 && (
                    <span className="text-[10px] text-[var(--color-text-tertiary)]">
                      +{agent.allowedTools.length - 8}
                    </span>
                  )}
                </div>
              )}
              {agent.agentic && (
                <span className="text-[10px] uppercase tracking-wide rounded bg-[var(--color-info-container)] text-[var(--color-info)] px-1.5 py-[1px]">
                  {t('settings.subagents.agenticBadge')}
                </span>
              )}
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={Boolean(pendingDelete)}
        title={t('settings.subagents.confirmDeleteTitle')}
        body={
          pendingDelete
            ? t('settings.subagents.confirmDeleteMessage').replace(
                '{name}',
                pendingDelete.name,
              )
            : ''
        }
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        confirmVariant="danger"
        onConfirm={confirmDelete}
        onClose={() => setPendingDelete(null)}
      />
    </div>
  )
}
