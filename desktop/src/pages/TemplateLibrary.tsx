// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import Editor, { DiffEditor } from '@monaco-editor/react'
import { useTranslation } from '../i18n'
import { useUIStore } from '../stores/uiStore'
import { useTemplateLibraryStore } from '../stores/templateLibraryStore'
import { useDesignerStore } from '../stores/designerStore'
import {
  templateLibraryApi,
  type TemplateFileStatus,
  type TemplateItem,
  type TemplateKindId,
} from '../api/templateLibrary'
import { TemplateLibraryPreview } from '../components/templateLibrary/TemplateLibraryPreview'
import '../lib/monacoSetup'

type CategoryKey =
  | 'designSystems'
  | 'designerTemplates'
  | 'promptTemplates'
  | 'curatorTemplates'

const CATEGORIES: ReadonlyArray<{
  key: CategoryKey
  kind: TemplateKindId
  labelKey:
    | 'templateLibrary.category.designSystems'
    | 'templateLibrary.category.designerTemplates'
    | 'templateLibrary.category.promptTemplates'
    | 'templateLibrary.category.curatorTemplates'
  icon: string
}> = [
  {
    key: 'designSystems',
    kind: 'design-system',
    labelKey: 'templateLibrary.category.designSystems',
    icon: 'palette',
  },
  {
    key: 'designerTemplates',
    kind: 'designer-template',
    labelKey: 'templateLibrary.category.designerTemplates',
    icon: 'dashboard_customize',
  },
  {
    key: 'promptTemplates',
    kind: 'prompt-template',
    labelKey: 'templateLibrary.category.promptTemplates',
    icon: 'auto_awesome',
  },
  {
    key: 'curatorTemplates',
    kind: 'curator-template',
    labelKey: 'templateLibrary.category.curatorTemplates',
    icon: 'description',
  },
]

function itemKey(item: TemplateItem): string {
  return `${item.kind}|${item.id}|${item.surface ?? ''}`
}

function langFor(file: string): string {
  if (file.endsWith('.css')) return 'css'
  if (file.endsWith('.html') || file.endsWith('.htm')) return 'html'
  if (file.endsWith('.json')) return 'json'
  if (file.endsWith('.md') || file.endsWith('.markdown')) return 'markdown'
  return 'plaintext'
}

function StatusBadge({ status }: { status: TemplateFileStatus }) {
  const t = useTranslation()
  if (status === 'stale') {
    return (
      <span className="rounded px-1.5 py-0.5 text-[10px] font-medium bg-[var(--color-warning-surface,rgba(234,179,8,0.18))] text-[var(--color-warning,#b45309)]">
        {t('templateLibrary.badge.stale')}
      </span>
    )
  }
  if (status === 'customized') {
    return (
      <span className="rounded px-1.5 py-0.5 text-[10px] font-medium bg-[var(--color-surface-selected)] text-[var(--color-accent)]">
        {t('templateLibrary.badge.customized')}
      </span>
    )
  }
  if (status === 'user') {
    return (
      <span className="rounded px-1.5 py-0.5 text-[10px] font-medium bg-[var(--color-surface-selected)] text-[var(--color-accent)]">
        {t('templateLibrary.badge.user')}
      </span>
    )
  }
  return null
}

function ItemBadges({ item }: { item: TemplateItem }) {
  const t = useTranslation()
  return (
    <div className="flex flex-wrap items-center gap-1">
      <span className="rounded px-1.5 py-0.5 text-[10px] font-medium bg-[var(--color-surface-secondary)] text-[var(--color-text-tertiary)]">
        {item.source === 'user'
          ? t('templateLibrary.badge.user')
          : t('templateLibrary.badge.builtin')}
      </span>
      {item.source === 'builtin' && item.customized && !item.stale && (
        <span className="rounded px-1.5 py-0.5 text-[10px] font-medium bg-[var(--color-surface-selected)] text-[var(--color-accent)]">
          {t('templateLibrary.badge.customized')}
        </span>
      )}
      {item.stale && (
        <span className="rounded px-1.5 py-0.5 text-[10px] font-medium bg-[var(--color-warning-surface,rgba(234,179,8,0.18))] text-[var(--color-warning,#b45309)]">
          {t('templateLibrary.badge.stale')}
        </span>
      )}
    </div>
  )
}

function ConfirmOverlay({
  message,
  onConfirm,
  onCancel,
}: {
  message: string
  onConfirm: () => void
  onCancel: () => void
}) {
  const t = useTranslation()
  return (
    <div
      className="absolute inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onCancel}
    >
      <div
        className="w-[360px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <p className="mb-4 text-[13px] text-[var(--color-text-primary)]">{message}</p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md px-3 py-1.5 text-[12px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('templateLibrary.action.cancel')}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90"
          >
            {t('templateLibrary.action.delete')}
          </button>
        </div>
      </div>
    </div>
  )
}

function TemplateDetail({
  item,
  onBack,
  onChanged,
}: {
  item: TemplateItem
  onBack: () => void
  onChanged: () => void
}) {
  const t = useTranslation()
  const theme = useUIStore((s) => s.theme)
  const addToast = useUIStore((s) => s.addToast)
  const [buffers, setBuffers] = useState<Record<string, string>>({})
  const [activeFilePath, setActiveFilePath] = useState(item.files[0]?.path ?? '')
  const [saving, setSaving] = useState(false)
  const [diff, setDiff] = useState<{ builtin: string; current: string } | null>(null)
  const [confirm, setConfirm] = useState<null | { message: string; action: () => void }>(
    null,
  )

  const filesKey = useMemo(
    () => `${item.kind}|${item.id}|${item.surface ?? ''}|${item.files.length}`,
    [item],
  )

  useEffect(() => {
    let cancelled = false
    Promise.all(
      item.files.map((f) =>
        templateLibraryApi
          .file(f.path)
          .then((r) => [f.path, r.content] as const)
          .catch(() => [f.path, ''] as const),
      ),
    ).then((entries) => {
      if (cancelled) return
      const map: Record<string, string> = {}
      for (const [p, c] of entries) map[p] = c
      setBuffers(map)
      setActiveFilePath((prev) =>
        item.files.some((f) => f.path === prev) ? prev : item.files[0]?.path ?? '',
      )
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filesKey])

  const activeFile = item.files.find((f) => f.path === activeFilePath) ?? item.files[0] ?? null
  const editorTheme = theme === 'dark' ? 'vs-dark' : 'light'

  useEffect(() => {
    if (!diff && !confirm) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.stopPropagation()
      if (diff) setDiff(null)
      else if (confirm) setConfirm(null)
    }
    document.addEventListener('keydown', onKey, true)
    return () => document.removeEventListener('keydown', onKey, true)
  }, [diff, confirm])

  const handleSave = async () => {
    if (!activeFile) return
    setSaving(true)
    try {
      await templateLibraryApi.save(activeFile.path, buffers[activeFile.path] ?? '')
      addToast({ type: 'success', message: t('templateLibrary.toast.saved') })
      onChanged()
    } catch {
      addToast({ type: 'error', message: t('templateLibrary.toast.error') })
    } finally {
      setSaving(false)
    }
  }

  const handleReset = () => {
    if (!activeFile) return
    setConfirm({
      message: t('templateLibrary.confirmReset'),
      action: async () => {
        setConfirm(null)
        try {
          await templateLibraryApi.reset(activeFile.path)
          const fresh = await templateLibraryApi.file(activeFile.path)
          setBuffers((prev) => ({ ...prev, [activeFile.path]: fresh.content }))
          addToast({ type: 'success', message: t('templateLibrary.toast.reset') })
          onChanged()
        } catch {
          addToast({ type: 'error', message: t('templateLibrary.toast.error') })
        }
      },
    })
  }

  const handleDiff = async () => {
    if (!activeFile) return
    try {
      const builtin = await templateLibraryApi.builtinFile(activeFile.path)
      setDiff({ builtin: builtin.content, current: buffers[activeFile.path] ?? '' })
    } catch {
      addToast({ type: 'error', message: t('templateLibrary.toast.error') })
    }
  }

  const handleDelete = () => {
    setConfirm({
      message: t('templateLibrary.confirmDelete'),
      action: async () => {
        setConfirm(null)
        try {
          await templateLibraryApi.remove(item.kind, item.id, item.surface)
          addToast({ type: 'success', message: t('templateLibrary.toast.deleted') })
          onBack()
          onChanged()
        } catch {
          addToast({ type: 'error', message: t('templateLibrary.toast.error') })
        }
      },
    })
  }

  const canReset =
    activeFile != null &&
    (activeFile.status === 'customized' || activeFile.status === 'stale')

  return (
    <div className="relative flex h-full min-h-0 flex-1 flex-col">
      <div className="flex flex-shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-2">
        <button
          type="button"
          onClick={onBack}
          className="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          title={t('templateLibrary.action.back')}
        >
          <span className="material-symbols-outlined text-[18px]">arrow_back</span>
        </button>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-[14px] font-semibold text-[var(--color-text-primary)]">
              {item.name}
            </span>
            <ItemBadges item={item} />
          </div>
          <div className="truncate text-[11px] text-[var(--color-text-tertiary)]">
            {item.category}
            {item.id ? ` · ${item.id}` : ''}
          </div>
        </div>
        {item.source === 'user' && (
          <button
            type="button"
            onClick={handleDelete}
            className="flex h-7 items-center gap-1 rounded-md px-2 text-[12px] text-[var(--color-danger)] hover:bg-[var(--color-surface-hover)]"
          >
            <span className="material-symbols-outlined text-[16px]">delete</span>
            {t('templateLibrary.action.delete')}
          </button>
        )}
      </div>

      <div className="flex min-h-0 flex-1">
        <div className="flex min-h-0 w-1/2 flex-col border-r border-[var(--color-border)]">
          <div className="flex flex-shrink-0 items-center gap-1 overflow-x-auto border-b border-[var(--color-border)] px-2 py-1">
            {item.files.map((f) => (
              <button
                key={f.path}
                type="button"
                onClick={() => setActiveFilePath(f.path)}
                className={`flex flex-shrink-0 items-center gap-1 rounded px-2 py-1 text-[11px] ${
                  f.path === activeFilePath
                    ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                }`}
              >
                {f.file}
                <StatusBadge status={f.status} />
              </button>
            ))}
          </div>
          {activeFile?.status === 'stale' && (
            <div className="flex flex-shrink-0 items-center gap-2 bg-[var(--color-warning-surface,rgba(234,179,8,0.12))] px-3 py-1.5 text-[11px] text-[var(--color-text-secondary)]">
              <span className="material-symbols-outlined text-[14px]">info</span>
              <span className="flex-1">{t('templateLibrary.staleBanner')}</span>
            </div>
          )}
          <div className="min-h-0 flex-1">
            {activeFile && (
              <Editor
                key={activeFile.path}
                theme={editorTheme}
                language={langFor(activeFile.file)}
                value={buffers[activeFile.path] ?? ''}
                onChange={(value) =>
                  setBuffers((prev) => ({ ...prev, [activeFile.path]: value ?? '' }))
                }
                options={{
                  fontSize: 12,
                  minimap: { enabled: false },
                  scrollBeyondLastLine: false,
                  wordWrap: 'on',
                  automaticLayout: true,
                }}
              />
            )}
          </div>
          <div className="flex flex-shrink-0 items-center justify-end gap-2 border-t border-[var(--color-border)] px-3 py-2">
            {activeFile?.status === 'stale' && (
              <button
                type="button"
                onClick={handleDiff}
                className="rounded-md px-3 py-1.5 text-[12px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
              >
                {t('templateLibrary.action.diff')}
              </button>
            )}
            {canReset && (
              <button
                type="button"
                onClick={handleReset}
                className="rounded-md px-3 py-1.5 text-[12px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
              >
                {t('templateLibrary.action.reset')}
              </button>
            )}
            <button
              type="button"
              onClick={handleSave}
              disabled={saving}
              className="rounded-md bg-[var(--color-accent)] px-4 py-1.5 text-[12px] font-medium text-white hover:opacity-90 disabled:opacity-60"
            >
              {saving ? t('common.saving') : t('templateLibrary.action.save')}
            </button>
          </div>
        </div>
        <div className="min-h-0 w-1/2 overflow-hidden bg-[var(--color-surface)]">
          <TemplateLibraryPreview item={item} buffers={buffers} />
        </div>
      </div>

      {diff && (
        <div
          className="absolute inset-0 z-50 flex flex-col bg-black/50"
          onClick={() => setDiff(null)}
        >
          <div
            className="m-6 flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex flex-shrink-0 items-center justify-between border-b border-[var(--color-border)] px-4 py-2">
              <span className="text-[13px] font-medium text-[var(--color-text-primary)]">
                {t('templateLibrary.diff.title')}
              </span>
              <button
                type="button"
                onClick={() => setDiff(null)}
                className="flex h-7 w-7 items-center justify-center rounded-md text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]"
              >
                <span className="material-symbols-outlined text-[18px]">close</span>
              </button>
            </div>
            <div className="min-h-0 flex-1">
              <DiffEditor
                theme={editorTheme}
                language={activeFile ? langFor(activeFile.file) : 'plaintext'}
                original={diff.builtin}
                modified={diff.current}
                options={{
                  readOnly: true,
                  fontSize: 12,
                  minimap: { enabled: false },
                  automaticLayout: true,
                }}
              />
            </div>
          </div>
        </div>
      )}

      {confirm && (
        <ConfirmOverlay
          message={confirm.message}
          onConfirm={confirm.action}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  )
}

function CreateForm({
  kind,
  onClose,
  onCreated,
}: {
  kind: TemplateKindId
  onClose: () => void
  onCreated: (id: string, surface?: string) => void
}) {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)
  const [id, setId] = useState('')
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [category, setCategory] = useState('')
  const [surface, setSurface] = useState('image')
  const [baseKind, setBaseKind] = useState('solution_functional')
  const [busy, setBusy] = useState(false)

  const submit = async () => {
    const cleanId = id.trim()
    if (!cleanId) return
    setBusy(true)
    try {
      await templateLibraryApi.create({
        kind,
        id: cleanId,
        name: name.trim() || undefined,
        description: description.trim() || undefined,
        category: category.trim() || undefined,
        surface: kind === 'prompt-template' ? surface : undefined,
        base_kind: kind === 'curator-template' ? baseKind : undefined,
      })
      addToast({ type: 'success', message: t('templateLibrary.toast.created') })
      onCreated(cleanId, kind === 'prompt-template' ? surface : undefined)
    } catch {
      addToast({ type: 'error', message: t('templateLibrary.toast.error') })
    } finally {
      setBusy(false)
    }
  }

  const inputClass =
    'w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]'

  return (
    <div
      className="absolute inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="w-[420px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-3 text-[14px] font-semibold text-[var(--color-text-primary)]">
          {t('templateLibrary.create.title')}
        </div>
        <div className="flex flex-col gap-2.5">
          <label className="flex flex-col gap-1">
            <span className="text-[11px] text-[var(--color-text-tertiary)]">
              {t('templateLibrary.create.id')}
            </span>
            <input
              className={inputClass}
              value={id}
              onChange={(e) => setId(e.target.value)}
              placeholder="my-template"
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[11px] text-[var(--color-text-tertiary)]">
              {t('templateLibrary.create.name')}
            </span>
            <input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[11px] text-[var(--color-text-tertiary)]">
              {t('templateLibrary.create.description')}
            </span>
            <input
              className={inputClass}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </label>
          {kind !== 'curator-template' && (
            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-[var(--color-text-tertiary)]">
                {t('templateLibrary.create.category')}
              </span>
              <input
                className={inputClass}
                value={category}
                onChange={(e) => setCategory(e.target.value)}
              />
            </label>
          )}
          {kind === 'prompt-template' && (
            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-[var(--color-text-tertiary)]">
                {t('templateLibrary.create.surface')}
              </span>
              <select
                className={inputClass}
                value={surface}
                onChange={(e) => setSurface(e.target.value)}
              >
                <option value="image">image</option>
                <option value="video">video</option>
              </select>
            </label>
          )}
          {kind === 'curator-template' && (
            <label className="flex flex-col gap-1">
              <span className="text-[11px] text-[var(--color-text-tertiary)]">
                {t('templateLibrary.create.baseKind')}
              </span>
              <input
                className={inputClass}
                value={baseKind}
                onChange={(e) => setBaseKind(e.target.value)}
              />
            </label>
          )}
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-[12px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('templateLibrary.action.cancel')}
          </button>
          <button
            type="button"
            onClick={submit}
            disabled={busy || !id.trim()}
            className="rounded-md bg-[var(--color-accent)] px-4 py-1.5 text-[12px] font-medium text-white hover:opacity-90 disabled:opacity-60"
          >
            {t('templateLibrary.create.submit')}
          </button>
        </div>
      </div>
    </div>
  )
}

export function TemplateLibrary() {
  const t = useTranslation()
  const close = useUIStore((s) => s.closeTemplateLibrary)
  const { catalog, loading, error, load } = useTemplateLibraryStore()
  const [activeCategory, setActiveCategory] = useState<CategoryKey>('designSystems')
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (creating) {
        setCreating(false)
        return
      }
      close()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [close, creating])

  const items = useMemo(() => {
    if (!catalog) return [] as TemplateItem[]
    return catalog[activeCategory] ?? []
  }, [catalog, activeCategory])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return items
    return items.filter(
      (it) =>
        it.name.toLowerCase().includes(q) ||
        it.id.toLowerCase().includes(q) ||
        it.category.toLowerCase().includes(q),
    )
  }, [items, search])

  const selectedItem = useMemo(
    () => items.find((it) => itemKey(it) === selectedKey) ?? null,
    [items, selectedKey],
  )

  const activeKind = CATEGORIES.find((c) => c.key === activeCategory)?.kind ?? 'design-system'

  return (
    <div className="flex h-full flex-col bg-[var(--color-surface)]">
      <div className="flex flex-shrink-0 items-center justify-between border-b border-[var(--color-border)] px-4 py-3">
        <div className="flex min-w-0 flex-col">
          <span className="text-[15px] font-semibold text-[var(--color-text-primary)]">
            {t('templateLibrary.title')}
          </span>
          <span className="truncate text-[11px] text-[var(--color-text-tertiary)]">
            {t('templateLibrary.subtitle')}
          </span>
        </div>
        <button
          type="button"
          onClick={close}
          title={t('templateLibrary.close')}
          className="flex h-8 w-8 items-center justify-center rounded-full text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[18px]">close</span>
        </button>
      </div>

      <div className="flex min-h-0 flex-1">
        <div className="flex w-[200px] flex-shrink-0 flex-col border-r border-[var(--color-border)] py-2">
          {CATEGORIES.map((cat) => {
            const count = catalog ? (catalog[cat.key] ?? []).length : 0
            return (
              <button
                key={cat.key}
                type="button"
                onClick={() => {
                  setActiveCategory(cat.key)
                  setSelectedKey(null)
                  setSearch('')
                }}
                className={`mx-2 flex items-center gap-2 rounded-md px-2.5 py-2 text-left text-[13px] ${
                  activeCategory === cat.key
                    ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                }`}
              >
                <span className="material-symbols-outlined text-[18px]">{cat.icon}</span>
                <span className="flex-1 truncate">{t(cat.labelKey)}</span>
                <span className="text-[11px] text-[var(--color-text-tertiary)]">{count}</span>
              </button>
            )
          })}
        </div>

        <div className="flex min-h-0 flex-1 flex-col">
          {selectedItem ? (
            <TemplateDetail
              item={selectedItem}
              onBack={() => setSelectedKey(null)}
              onChanged={() => {
                void load()
                void useDesignerStore.getState().refresh()
              }}
            />
          ) : (
            <>
              <div className="flex flex-shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-2">
                <div className="relative flex-1">
                  <span className="material-symbols-outlined pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-[16px] text-[var(--color-text-tertiary)]">
                    search
                  </span>
                  <input
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    placeholder={t('templateLibrary.search')}
                    className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] py-1.5 pl-8 pr-2 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
                  />
                </div>
                <button
                  type="button"
                  onClick={() => setCreating(true)}
                  className="flex items-center gap-1 rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90"
                >
                  <span className="material-symbols-outlined text-[16px]">add</span>
                  {t('templateLibrary.action.create')}
                </button>
              </div>
              <div className="min-h-0 flex-1 overflow-auto p-4">
                {loading && (
                  <div className="flex h-40 items-center justify-center text-[12px] text-[var(--color-text-tertiary)]">
                    {t('templateLibrary.loading')}
                  </div>
                )}
                {!loading && error && (
                  <div className="flex h-40 items-center justify-center text-[12px] text-[var(--color-danger)]">
                    {error}
                  </div>
                )}
                {!loading && !error && filtered.length === 0 && (
                  <div className="flex h-40 items-center justify-center text-[12px] text-[var(--color-text-tertiary)]">
                    {t('templateLibrary.empty')}
                  </div>
                )}
                <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
                  {filtered.map((item) => (
                    <button
                      key={itemKey(item)}
                      type="button"
                      onClick={() => setSelectedKey(itemKey(item))}
                      className="flex flex-col gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-left transition-shadow hover:shadow-md"
                    >
                      <div className="flex items-start justify-between gap-2">
                        <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-[var(--color-text-primary)]">
                          {item.name}
                        </span>
                      </div>
                      <span className="line-clamp-2 min-h-[2.4em] text-[11px] text-[var(--color-text-tertiary)]">
                        {item.description}
                      </span>
                      <div className="flex items-center justify-between gap-2">
                        <span className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                          {item.category}
                        </span>
                        <ItemBadges item={item} />
                      </div>
                    </button>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      </div>

      {creating && (
        <CreateForm
          kind={activeKind}
          onClose={() => setCreating(false)}
          onCreated={(id, surface) => {
            setCreating(false)
            void load()
            void useDesignerStore.getState().refresh()
            setSelectedKey(`${activeKind}|${id}|${surface ?? ''}`)
          }}
        />
      )}
    </div>
  )
}
