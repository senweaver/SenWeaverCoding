import { useEffect, useMemo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'

import { Button } from '../components/shared/Button'
import { ConfirmDialog } from '../components/shared/ConfirmDialog'
import { Input } from '../components/shared/Input'
import { Modal } from '../components/shared/Modal'
import { useTranslation } from '../i18n'
import {
  useCredentialsStore,
} from '../stores/credentialsStore'
import type { CredentialKind, CredentialMeta } from '../api/credentials'

const KIND_OPTIONS: CredentialKind[] = [
  'username',
  'password',
  'token',
  'url',
  'other',
]

const NAME_PATTERN = /^[A-Za-z0-9_-]+$/

function formatTimestamp(
  value: string | number | null | undefined,
): string {
  if (value === null || value === undefined || value === '') return '—'
  let ms: number
  if (typeof value === 'number') {
    if (value <= 0) return '—'
    ms = value < 1e12 ? value * 1000 : value
  } else {
    const asNumber = Number(value)
    if (Number.isFinite(asNumber) && /^\d+$/.test(value.trim())) {
      ms = asNumber < 1e12 ? asNumber * 1000 : asNumber
    } else {
      const parsed = Date.parse(value)
      if (Number.isNaN(parsed)) return value
      ms = parsed
    }
  }
  const d = new Date(ms)
  if (Number.isNaN(d.getTime())) return String(value)
  return d.toLocaleString()
}

export function CredentialsSettings() {
  const t = useTranslation()
  const { credentials, isLoading, error, hasFetched, fetchAll, upsert, remove } =
    useCredentialsStore(
      useShallow((s) => ({
        credentials: s.credentials,
        isLoading: s.isLoading,
        error: s.error,
        hasFetched: s.hasFetched,
        fetchAll: s.fetchAll,
        upsert: s.upsert,
        remove: s.remove,
      })),
    )

  const [editing, setEditing] = useState<CredentialMeta | null>(null)
  const [showAdd, setShowAdd] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<CredentialMeta | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  useEffect(() => {
    if (!hasFetched && !isLoading) {
      void fetchAll()
    }
  }, [hasFetched, isLoading, fetchAll])

  const tKind = (kind: CredentialKind) => t(`credentials.kind.${kind}` as const)

  const sorted = useMemo(
    () => [...credentials].sort((a, b) => a.name.localeCompare(b.name)),
    [credentials],
  )

  const confirmDelete = async () => {
    if (!pendingDelete) return
    setIsDeleting(true)
    try {
      await remove(pendingDelete.name)
      setPendingDelete(null)
    } finally {
      setIsDeleting(false)
    }
  }

  return (
    <div>
      <div className="flex items-center justify-between gap-3 mb-4">
        <div className="min-w-0">
          <h2 className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t('credentials.title')}
          </h2>
          <p className="text-xs text-[var(--color-text-tertiary)] mt-0.5 max-w-xl">
            {t('credentials.description')}
          </p>
        </div>
        <Button size="sm" className="shrink-0" onClick={() => setShowAdd(true)}>
          <span className="material-symbols-outlined text-[14px]">add</span>
          {t('credentials.add')}
        </Button>
      </div>

      {error ? (
        <div className="mb-3 text-xs text-[var(--color-error)] px-3 py-2 rounded-[var(--radius-md)] bg-[color:rgba(239,68,68,0.08)] border border-[color:rgba(239,68,68,0.25)]">
          {error}
        </div>
      ) : null}

      {isLoading && credentials.length === 0 ? (
        <div className="flex justify-center py-8">
          <div className="animate-spin w-5 h-5 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
        </div>
      ) : sorted.length === 0 ? (
        <div className="text-xs text-[var(--color-text-tertiary)] py-6 text-center border border-dashed border-[var(--color-border)] rounded-xl">
          {t('credentials.empty')}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {sorted.map((cred) => (
            <div
              key={cred.name}
              className="flex items-center gap-2 px-3 py-2.5 rounded-xl border border-[var(--color-border)] hover:border-[var(--color-border-focus)] transition-colors"
            >
              <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)]">
                key
              </span>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-mono font-semibold text-[var(--color-text-primary)] truncate">
                    {cred.name}
                  </span>
                  <span className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] leading-none">
                    {tKind(cred.kind)}
                  </span>
                </div>
                <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
                  {t('credentials.updated')}: {formatTimestamp(cred.updated_at)}
                </div>
              </div>
              <Button variant="ghost" size="sm" onClick={() => setEditing(cred)}>
                {t('credentials.edit')}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setPendingDelete(cred)}
                className="text-[var(--color-error)] hover:text-[var(--color-error)]"
              >
                {t('credentials.delete')}
              </Button>
            </div>
          ))}
        </div>
      )}

      {showAdd ? (
        <CredentialFormModal
          mode="create"
          onClose={() => setShowAdd(false)}
          onSubmit={async (input) => {
            await upsert(input)
            setShowAdd(false)
          }}
        />
      ) : null}

      {editing ? (
        <CredentialFormModal
          mode="edit"
          existing={editing}
          onClose={() => setEditing(null)}
          onSubmit={async (input) => {
            await upsert(input)
            setEditing(null)
          }}
        />
      ) : null}

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => {
          if (isDeleting) return
          setPendingDelete(null)
        }}
        onConfirm={confirmDelete}
        title={t('credentials.delete')}
        body={
          pendingDelete
            ? t('credentials.deleteConfirm').replace('{name}', pendingDelete.name)
            : ''
        }
        confirmLabel={t('credentials.delete')}
        cancelLabel={t('credentials.cancel')}
        confirmVariant="danger"
        loading={isDeleting}
      />
    </div>
  )
}

type FormProps = {
  mode: 'create' | 'edit'
  existing?: CredentialMeta
  onClose: () => void
  onSubmit: (input: {
    name: string
    kind: CredentialKind
    value: string
  }) => Promise<void>
}

function CredentialFormModal({ mode, existing, onClose, onSubmit }: FormProps) {
  const t = useTranslation()
  const [name, setName] = useState(existing?.name ?? '')
  const [kind, setKind] = useState<CredentialKind>(existing?.kind ?? 'password')
  const [value, setValue] = useState('')
  const [showValue, setShowValue] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const nameValid = NAME_PATTERN.test(name.trim())
  const canSubmit = name.trim().length > 0 && nameValid && value.length > 0 && !submitting

  const handleSubmit = async () => {
    if (!canSubmit) return
    setSubmitting(true)
    setError(null)
    try {
      await onSubmit({ name: name.trim(), kind, value })
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={mode === 'create' ? t('credentials.add') : t('credentials.edit')}
      width={520}
      footer={
        <>
          <Button variant="secondary" size="sm" onClick={onClose}>
            {t('credentials.cancel')}
          </Button>
          <Button size="sm" onClick={handleSubmit} disabled={!canSubmit} loading={submitting}>
            {t('credentials.save')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-xs">
        <Input
          label={t('credentials.name')}
          required
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="example_token"
          disabled={mode === 'edit'}
          error={!nameValid && name.length > 0 ? t('credentials.nameInvalid') : undefined}
        />

        <div>
          <label className="text-xs font-medium text-[var(--color-text-primary)] mb-1 block">
            {t('credentials.kind')}
          </label>
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as CredentialKind)}
            className="w-full h-8 px-2.5 rounded-[var(--radius-md)] bg-[var(--color-surface)] border border-[var(--color-border)] text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
          >
            {KIND_OPTIONS.map((opt) => (
              <option key={opt} value={opt}>
                {t(`credentials.kind.${opt}` as const)}
              </option>
            ))}
          </select>
        </div>

        <div className="flex items-end gap-2">
          <div className="flex-1">
            <Input
              label={t('credentials.value')}
              required
              type={showValue ? 'text' : 'password'}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder={mode === 'edit' ? '****' : ''}
            />
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setShowValue((s) => !s)}
            type="button"
          >
            {showValue ? t('credentials.hide') : t('credentials.show')}
          </Button>
        </div>

        {error ? (
          <div className="text-xs text-[var(--color-error)] px-3 py-2 rounded-[var(--radius-md)] bg-[color:rgba(239,68,68,0.08)] border border-[color:rgba(239,68,68,0.25)]">
            {error}
          </div>
        ) : null}
      </div>
    </Modal>
  )
}
