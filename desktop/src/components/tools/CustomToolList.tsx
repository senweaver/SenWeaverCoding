import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'
import { ConfirmDialog } from '../shared/ConfirmDialog'
import { useCustomToolsStore } from '../../stores/customToolsStore'
import { useUIStore } from '../../stores/uiStore'
import { CustomToolEditor } from './CustomToolEditor'
import type { CustomToolDef } from '../../types/customTools'

type Mode = { kind: 'list' } | { kind: 'create' } | { kind: 'edit'; tool: CustomToolDef }

export function CustomToolList() {
  const t = useTranslation()
  const tools = useCustomToolsStore((s) => s.tools)
  const isLoading = useCustomToolsStore((s) => s.isLoading)
  const isSaving = useCustomToolsStore((s) => s.isSaving)
  const error = useCustomToolsStore((s) => s.error)
  const fetch = useCustomToolsStore((s) => s.fetch)
  const create = useCustomToolsStore((s) => s.create)
  const update = useCustomToolsStore((s) => s.update)
  const remove = useCustomToolsStore((s) => s.remove)
  const addToast = useUIStore((s) => s.addToast)
  const [mode, setMode] = useState<Mode>({ kind: 'list' })
  const [pendingDelete, setPendingDelete] = useState<CustomToolDef | null>(null)

  useEffect(() => {
    void fetch()
  }, [fetch])

  async function handleCreate(def: CustomToolDef) {
    await create(def)
    addToast({ type: 'success', message: t('settings.tools.createdToast') })
    setMode({ kind: 'list' })
  }

  async function handleUpdate(def: CustomToolDef) {
    if (mode.kind !== 'edit') return
    await update(mode.tool.name, def)
    addToast({ type: 'success', message: t('settings.tools.updatedToast') })
    setMode({ kind: 'list' })
  }

  async function confirmDelete() {
    if (!pendingDelete) return
    try {
      await remove(pendingDelete.name)
      addToast({ type: 'success', message: t('settings.tools.deletedToast') })
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
      <CustomToolEditor
        initial={mode.kind === 'edit' ? mode.tool : null}
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
          {t('settings.tools.listDescription')}
        </p>
        <Button size="sm" onClick={() => setMode({ kind: 'create' })}>
          <span className="material-symbols-outlined text-[14px] mr-1">add</span>
          {t('settings.tools.create')}
        </Button>
      </div>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      {tools.length === 0 && !isLoading ? (
        <div className="rounded-lg border border-dashed border-[var(--color-border)] p-4 text-center text-xs text-[var(--color-text-secondary)]">
          {t('settings.tools.empty')}
        </div>
      ) : (
        <ul className="space-y-2">
          {tools.map((tool) => (
            <li
              key={tool.name}
              className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-xs font-semibold text-[var(--color-text-primary)] flex items-center gap-2">
                    <code className="font-mono text-xs">custom_{tool.name}</code>
                    {!tool.enabled && (
                      <span className="text-[10px] uppercase tracking-wide rounded bg-[var(--color-surface-container-high)] px-1.5 py-[1px] text-[var(--color-text-tertiary)]">
                        {t('settings.tools.disabled')}
                      </span>
                    )}
                  </p>
                  {tool.description && (
                    <p className="text-xs text-[var(--color-text-secondary)] mt-1">
                      {tool.description}
                    </p>
                  )}
                  <p className="text-xs text-[var(--color-text-tertiary)] mt-1 font-mono truncate">
                    {[tool.command, ...tool.args].join(' ')}
                  </p>
                </div>
                <div className="flex items-center gap-1 flex-shrink-0">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setMode({ kind: 'edit', tool })}
                  >
                    <span className="material-symbols-outlined text-[14px]">edit</span>
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setPendingDelete(tool)}
                  >
                    <span className="material-symbols-outlined text-[14px]">delete</span>
                  </Button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={Boolean(pendingDelete)}
        title={t('settings.tools.confirmDeleteTitle')}
        body={
          pendingDelete
            ? t('settings.tools.confirmDeleteMessage').replace('{name}', pendingDelete.name)
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
