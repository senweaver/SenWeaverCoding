// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useEffect, useMemo, useCallback } from 'react'
import { useAdapterStore } from '../stores/adapterStore'
import { useTranslation } from '../i18n'
import type { TranslationKey } from '../i18n/locales/en'
import { Button } from '../components/shared/Button'
import { DirectoryPicker } from '../components/shared/DirectoryPicker'
import { ConfirmDialog } from '../components/shared/ConfirmDialog'
import { ChannelDetailForm } from '../components/adapters/ChannelDetailForm'
import { SupportedChannels } from '../components/adapters/SupportedChannels'
import { IntegrationCatalog } from '../components/adapters/IntegrationCatalog'
import {
  CHANNEL_CATEGORIES,
  CHANNEL_DEFINITIONS,
  SESSION_BACKEND_CHOICES,
  channelsByCategory,
  type ChannelDefinition,
} from '../components/adapters/channelDefinitions'
import type {
  AdapterFileConfig,
  ChannelId,
  GlobalChannelConfig,
  PairingChannelId,
  PairedUser,
} from '../types/adapter'
import { PAIRING_CHANNELS } from '../types/adapter'

type SaveStatus = 'idle' | 'saved' | 'error'

export function AdapterSettings() {
  const t = useTranslation()
  const config = useAdapterStore((s) => s.config)
  const isLoading = useAdapterStore((s) => s.isLoading)
  const fetchConfig = useAdapterStore((s) => s.fetchConfig)
  const updateConfig = useAdapterStore((s) => s.updateConfig)
  const disableChannel = useAdapterStore((s) => s.disableChannel)
  const generatePairingCode = useAdapterStore((s) => s.generatePairingCode)
  const removePairedUser = useAdapterStore((s) => s.removePairedUser)

  const [defaultProjectDir, setDefaultProjectDir] = useState('')
  const [globalState, setGlobalState] = useState<GlobalChannelConfig>({})

  const [drafts, setDrafts] = useState<Record<string, Record<string, unknown>>>({})
  const [openChannel, setOpenChannel] = useState<ChannelId | null>(null)
  const [saving, setSaving] = useState<string | null>(null)
  const [saveStatus, setSaveStatus] = useState<Record<string, SaveStatus>>({})
  const [saveError, setSaveError] = useState<Record<string, string>>({})

  const [pairingCode, setPairingCode] = useState<string | null>(null)
  const [isGeneratingCode, setIsGeneratingCode] = useState(false)
  const [pendingUnbind, setPendingUnbind] = useState<{
    platform: PairingChannelId
    userId: string | number
  } | null>(null)
  const [isUnbinding, setIsUnbinding] = useState(false)

  const [pendingDisable, setPendingDisable] = useState<ChannelId | null>(null)

  useEffect(() => {
    void fetchConfig()
  }, [fetchConfig])

  useEffect(() => {
    setDefaultProjectDir(config.defaultProjectDir ?? '')
    setGlobalState(config.global ?? {})
    const next: Record<string, Record<string, unknown>> = {}
    for (const def of CHANNEL_DEFINITIONS) {
      const channelValue = (config as AdapterFileConfig)[def.id]
      next[def.id] = (channelValue as Record<string, unknown> | null | undefined) ?? {}
    }
    setDrafts(next)
  }, [config])

  const features = config.features ?? {}

  async function saveTopSection() {
    setSaving('__top')
    setSaveStatus((s) => ({ ...s, __top: 'idle' }))
    try {
      const patch: Record<string, unknown> = {}
      if (defaultProjectDir.trim()) patch.defaultProjectDir = defaultProjectDir.trim()
      patch.global = stripUndefined(globalState)
      await updateConfig(patch)
      setSaveStatus((s) => ({ ...s, __top: 'saved' }))
      setTimeout(() => setSaveStatus((s) => ({ ...s, __top: 'idle' })), 2000)
    } catch (err) {
      setSaveStatus((s) => ({ ...s, __top: 'error' }))
      setSaveError((s) => ({ ...s, __top: err instanceof Error ? err.message : 'Save failed' }))
    } finally {
      setSaving(null)
    }
  }

  async function saveChannel(channel: ChannelId) {
    setSaving(channel)
    setSaveStatus((s) => ({ ...s, [channel]: 'idle' }))
    setSaveError((s) => ({ ...s, [channel]: '' }))
    try {
      const draft = drafts[channel] ?? {}
      const cleaned = stripUndefined(draft)
      await updateConfig({ [channel]: cleaned })
      setSaveStatus((s) => ({ ...s, [channel]: 'saved' }))
      setTimeout(() => setSaveStatus((s) => ({ ...s, [channel]: 'idle' })), 2000)
    } catch (err) {
      setSaveStatus((s) => ({ ...s, [channel]: 'error' }))
      setSaveError((s) => ({
        ...s,
        [channel]: err instanceof Error ? err.message : 'Save failed',
      }))
    } finally {
      setSaving(null)
    }
  }

  async function confirmDisable() {
    if (!pendingDisable) return
    const ch = pendingDisable
    setPendingDisable(null)
    setSaving(ch)
    try {
      await disableChannel(ch)
      setSaveStatus((s) => ({ ...s, [ch]: 'saved' }))
      setTimeout(() => setSaveStatus((s) => ({ ...s, [ch]: 'idle' })), 2000)
    } catch (err) {
      setSaveStatus((s) => ({ ...s, [ch]: 'error' }))
      setSaveError((s) => ({ ...s, [ch]: err instanceof Error ? err.message : 'Disable failed' }))
    } finally {
      setSaving(null)
    }
  }

  const handleGenerateCode = useCallback(async () => {
    setIsGeneratingCode(true)
    try {
      const code = await generatePairingCode()
      setPairingCode(code)
    } catch (err) {
      console.error('Failed to generate pairing code:', err)
    } finally {
      setIsGeneratingCode(false)
    }
  }, [generatePairingCode])

  const allPairedUsers = useMemo(() => {
    const out: Array<PairedUser & { platform: PairingChannelId }> = []
    for (const platform of PAIRING_CHANNELS) {
      const cfg = (config as AdapterFileConfig)[platform] as
        | { pairedUsers?: PairedUser[] }
        | null
        | undefined
      const users = cfg?.pairedUsers ?? []
      for (const u of users) {
        out.push({ ...u, platform })
      }
    }
    return out
  }, [config])

  const pairingExpiry = config.pairing?.expiresAt
  const isPairingActive = pairingExpiry ? Date.now() < pairingExpiry : false
  const minutesLeft = pairingExpiry
    ? Math.max(0, Math.ceil((pairingExpiry - Date.now()) / 60000))
    : 0

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12 text-xs text-[var(--color-text-tertiary)]">
        <span className="material-symbols-outlined animate-spin text-[18px] mr-2">
          progress_activity
        </span>
        {t('common.loading')}
      </div>
    )
  }

  return (
    <div className="space-y-4">
      {}
      <p className="text-xs text-[var(--color-text-secondary)]">
        {t('settings.adapters.descriptionFull')}
      </p>

      <SupportedChannels />

      {}
      <section className="rounded-xl border border-[var(--color-border)] overflow-hidden">
        <SectionHeader
          icon="tune"
          label={t('settings.adapters.generalSection')}
          desc={t('settings.adapters.generalSectionDesc')}
        />
        <div className="p-3 space-y-4">
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-[var(--color-text-primary)]">
              {t('settings.adapters.defaultProject')}
            </label>
            <DirectoryPicker value={defaultProjectDir} onChange={setDefaultProjectDir} />
            <p className="text-xs text-[var(--color-text-tertiary)]">
              {t('settings.adapters.defaultProjectHint')}
            </p>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <NumberField
              label={t('settings.adapters.fields.messageTimeoutSecs')}
              hint={t('settings.adapters.hints.messageTimeoutSecs')}
              value={globalState.messageTimeoutSecs}
              onChange={(v) => setGlobalState((s) => ({ ...s, messageTimeoutSecs: v }))}
            />
            <NumberField
              label={t('settings.adapters.fields.debounceMs')}
              hint={t('settings.adapters.hints.debounceMs')}
              value={globalState.debounceMs}
              onChange={(v) => setGlobalState((s) => ({ ...s, debounceMs: v }))}
            />
            <SelectField
              label={t('settings.adapters.fields.sessionBackend')}
              value={globalState.sessionBackend ?? ''}
              options={SESSION_BACKEND_CHOICES.map((o) => ({
                value: o.value,
                label: t(o.i18nLabel as TranslationKey),
              }))}
              onChange={(v) => setGlobalState((s) => ({ ...s, sessionBackend: v || undefined }))}
            />
            <NumberField
              label={t('settings.adapters.fields.sessionTtlHours')}
              hint={t('settings.adapters.hints.sessionTtlHours')}
              value={globalState.sessionTtlHours}
              onChange={(v) => setGlobalState((s) => ({ ...s, sessionTtlHours: v }))}
            />
          </div>

          <div className="grid grid-cols-2 gap-2">
            <CheckboxField
              label={t('settings.adapters.fields.cli')}
              hint={t('settings.adapters.hints.cli')}
              value={globalState.cli ?? true}
              onChange={(v) => setGlobalState((s) => ({ ...s, cli: v }))}
            />
            <CheckboxField
              label={t('settings.adapters.fields.ackReactions')}
              hint={t('settings.adapters.hints.ackReactions')}
              value={globalState.ackReactions ?? true}
              onChange={(v) => setGlobalState((s) => ({ ...s, ackReactions: v }))}
            />
            <CheckboxField
              label={t('settings.adapters.fields.showToolCalls')}
              hint={t('settings.adapters.hints.showToolCalls')}
              value={globalState.showToolCalls ?? false}
              onChange={(v) => setGlobalState((s) => ({ ...s, showToolCalls: v }))}
            />
            <CheckboxField
              label={t('settings.adapters.fields.sessionPersistence')}
              hint={t('settings.adapters.hints.sessionPersistence')}
              value={globalState.sessionPersistence ?? true}
              onChange={(v) => setGlobalState((s) => ({ ...s, sessionPersistence: v }))}
            />
          </div>

          <div className="flex items-center gap-3 pt-1">
            <Button size="sm" onClick={saveTopSection} loading={saving === '__top'}>
              {t('settings.adapters.save')}
            </Button>
            {saveStatus['__top'] === 'saved' && (
              <span className="text-xs text-[var(--color-success)]">
                <span className="material-symbols-outlined text-[14px] align-middle mr-1">
                  check_circle
                </span>
                {t('settings.adapters.saved')}
              </span>
            )}
            {saveStatus['__top'] === 'error' && (
              <span className="text-xs text-[var(--color-error)]">
                <span className="material-symbols-outlined text-[14px] align-middle mr-1">
                  error
                </span>
                {saveError['__top']}
              </span>
            )}
          </div>
        </div>
      </section>

      {}
      <section className="rounded-xl border border-[var(--color-border)] overflow-hidden">
        <SectionHeader
          icon="link"
          label={t('settings.adapters.pairing')}
          desc={t('settings.adapters.pairingDesc')}
        />
        <div className="p-3 space-y-4">
          <div className="flex items-center gap-3">
            <Button size="sm" onClick={handleGenerateCode} loading={isGeneratingCode}>
              {pairingCode || isPairingActive
                ? t('settings.adapters.regenerateCode')
                : t('settings.adapters.generateCode')}
            </Button>
            {pairingCode && (
              <div className="flex items-center gap-2">
                <span className="font-mono text-xs font-bold tracking-[0.3em] text-[var(--color-brand)]">
                  {pairingCode}
                </span>
                <span className="text-xs text-[var(--color-text-tertiary)]">
                  {t('settings.adapters.codeExpiresIn')} 60 {t('settings.adapters.minutes')}
                </span>
              </div>
            )}
            {!pairingCode && isPairingActive && (
              <span className="text-xs text-[var(--color-text-tertiary)]">
                {t('settings.adapters.codeExpiresIn')} {minutesLeft}{' '}
                {t('settings.adapters.minutes')}
              </span>
            )}
          </div>

          <div>
            <h4 className="text-xs font-medium text-[var(--color-text-primary)] mb-2">
              {t('settings.adapters.pairedUsers')}
            </h4>
            {allPairedUsers.length === 0 ? (
              <p className="text-xs text-[var(--color-text-tertiary)]">
                {t('settings.adapters.noPairedUsers')}
              </p>
            ) : (
              <div className="space-y-2">
                {allPairedUsers.map((user) => (
                  <div
                    key={`${user.platform}-${user.userId}`}
                    className="flex items-center justify-between px-3 py-2 rounded-lg bg-[var(--color-surface-hover)]"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-xs px-1.5 py-0.5 rounded bg-[var(--color-surface)] text-[var(--color-text-secondary)]">
                        {t(`settings.adapters.platform.${user.platform}` as TranslationKey)}
                      </span>
                      <span className="text-xs text-[var(--color-text-primary)]">
                        {user.displayName}
                      </span>
                      <span className="text-xs text-[var(--color-text-tertiary)]">
                        {new Date(user.pairedAt).toLocaleDateString()}
                      </span>
                    </div>
                    <button
                      onClick={() => setPendingUnbind({ platform: user.platform, userId: user.userId })}
                      className="text-xs text-[var(--color-error)] hover:underline cursor-pointer"
                    >
                      {t('settings.adapters.unbind')}
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </section>

      {}
      {CHANNEL_CATEGORIES.map((cat) => {
        const list = channelsByCategory(cat.id)
        if (list.length === 0) return null
        return (
          <section key={cat.id} className="space-y-3">
            <header className="flex items-center gap-2">
              <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)] inline-flex items-center justify-center w-5 h-5 overflow-hidden">
                {cat.icon}
              </span>
              <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
                {t(cat.i18nLabel as TranslationKey)}
              </h3>
              <span className="text-xs text-[var(--color-text-tertiary)]">({list.length})</span>
            </header>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
              {list.map((def) => (
                <ChannelCard
                  key={def.id}
                  definition={def}
                  draft={drafts[def.id] ?? {}}
                  onDraftChange={(next) => setDrafts((s) => ({ ...s, [def.id]: next }))}
                  isOpen={openChannel === def.id}
                  onToggleOpen={() => setOpenChannel((s) => (s === def.id ? null : def.id))}
                  isConfigured={Boolean(
                    def.isConfigured((config as AdapterFileConfig)[def.id] as Parameters<typeof def.isConfigured>[0]),
                  )}
                  onSave={() => saveChannel(def.id)}
                  onDisable={() => setPendingDisable(def.id)}
                  isSaving={saving === def.id}
                  saveStatus={saveStatus[def.id] ?? 'idle'}
                  saveError={saveError[def.id]}
                  featureMissing={
                    def.featureFlag ? !features[def.featureFlag] : false
                  }
                />
              ))}
            </div>
          </section>
        )
      })}

      <IntegrationCatalog />

      <ConfirmDialog
        open={pendingUnbind !== null}
        onClose={() => {
          if (!isUnbinding) setPendingUnbind(null)
        }}
        onConfirm={async () => {
          if (!pendingUnbind) return
          setIsUnbinding(true)
          try {
            await removePairedUser(pendingUnbind.platform, pendingUnbind.userId)
            await fetchConfig()
            setPendingUnbind(null)
          } finally {
            setIsUnbinding(false)
          }
        }}
        title={t('settings.adapters.unbind')}
        body={t('settings.adapters.unbindConfirm')}
        confirmLabel={t('settings.adapters.unbind')}
        cancelLabel={t('common.cancel')}
        confirmVariant="danger"
        loading={isUnbinding}
      />

      <ConfirmDialog
        open={pendingDisable !== null}
        onClose={() => setPendingDisable(null)}
        onConfirm={confirmDisable}
        title={t('settings.adapters.disableChannel')}
        body={t('settings.adapters.disableChannelConfirm')}
        confirmLabel={t('settings.adapters.disable')}
        cancelLabel={t('common.cancel')}
        confirmVariant="danger"
      />
    </div>
  )
}

function SectionHeader({ icon, label, desc }: { icon: string; label: string; desc?: string }) {
  return (
    <div className="flex items-start gap-2 px-3 py-2 bg-[var(--color-surface-hover)] border-b border-[var(--color-border)]">
      <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)] mt-0.5">
        {icon}
      </span>
      <div className="flex-1 min-w-0">
        <div className="text-xs font-semibold text-[var(--color-text-primary)]">{label}</div>
        {desc && <p className="text-xs text-[var(--color-text-tertiary)]">{desc}</p>}
      </div>
    </div>
  )
}

type ChannelCardProps = {
  definition: ChannelDefinition
  draft: Record<string, unknown>
  onDraftChange: (next: Record<string, unknown>) => void
  isOpen: boolean
  onToggleOpen: () => void
  isConfigured: boolean
  onSave: () => void
  onDisable: () => void
  isSaving: boolean
  saveStatus: SaveStatus
  saveError?: string
  featureMissing: boolean
}

function ChannelCard({
  definition,
  draft,
  onDraftChange,
  isOpen,
  onToggleOpen,
  isConfigured,
  onSave,
  onDisable,
  isSaving,
  saveStatus,
  saveError,
  featureMissing,
}: ChannelCardProps) {
  const t = useTranslation()

  return (
    <div
      className={`rounded-xl border ${
        isOpen ? 'border-[var(--color-border-focus)]' : 'border-[var(--color-border)]'
      } bg-[var(--color-surface)] overflow-hidden transition-colors`}
    >
      <button
        type="button"
        onClick={onToggleOpen}
        className="w-full px-3 py-2.5 flex items-center gap-3 hover:bg-[var(--color-surface-hover)] transition-colors text-left cursor-pointer"
      >
        <span className="material-symbols-outlined text-[18px] text-[var(--color-text-secondary)] shrink-0 inline-flex items-center justify-center w-6 h-6 overflow-hidden">
          {definition.icon}
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium text-[var(--color-text-primary)] truncate">
              {t(definition.i18nName as TranslationKey)}
            </span>
            <StatusPill configured={isConfigured} disabled={featureMissing} />
            {definition.platformOnly && (
              <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-tertiary)]">
                {definition.platformOnly}
              </span>
            )}
          </div>
          {definition.i18nTagline && (
            <p className="text-xs text-[var(--color-text-tertiary)] truncate mt-0.5">
              {t(definition.i18nTagline as TranslationKey)}
            </p>
          )}
        </div>
        <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)] shrink-0">
          {isOpen ? 'expand_less' : 'expand_more'}
        </span>
      </button>

      {isOpen && (
        <div className="border-t border-[var(--color-border)] p-3 space-y-3 bg-[var(--color-surface-hover)]/30">
          {featureMissing && (
            <div className="flex items-start gap-2 rounded-lg border border-[var(--color-warning)]/30 bg-[var(--color-warning)]/5 px-3 py-2">
              <span className="material-symbols-outlined text-[16px] text-[var(--color-warning)]">
                warning
              </span>
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t('settings.adapters.featureMissing')}
              </p>
            </div>
          )}

          <ChannelDetailForm
            definition={definition}
            value={draft}
            onChange={onDraftChange}
            disabled={featureMissing}
          />

          <div className="flex items-center gap-3 pt-1 border-t border-[var(--color-border)]">
            <Button size="sm" onClick={onSave} loading={isSaving} disabled={featureMissing}>
              {t('settings.adapters.save')}
            </Button>
            {isConfigured && (
              <button
                type="button"
                onClick={onDisable}
                disabled={isSaving}
                className="text-xs text-[var(--color-error)] hover:underline cursor-pointer disabled:opacity-50"
              >
                {t('settings.adapters.disableChannel')}
              </button>
            )}
            {saveStatus === 'saved' && (
              <span className="text-xs text-[var(--color-success)]">
                <span className="material-symbols-outlined text-[14px] align-middle mr-1">
                  check_circle
                </span>
                {t('settings.adapters.saved')}
              </span>
            )}
            {saveStatus === 'error' && (
              <span className="text-xs text-[var(--color-error)] truncate">{saveError}</span>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

function StatusPill({ configured, disabled }: { configured: boolean; disabled: boolean }) {
  const t = useTranslation()
  if (disabled) {
    return (
      <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-tertiary)]">
        {t('settings.adapters.status.unavailable')}
      </span>
    )
  }
  if (configured) {
    return (
      <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-[var(--color-success)]/10 text-[var(--color-success)]">
        {t('settings.adapters.status.configured')}
      </span>
    )
  }
  return (
    <span className="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-tertiary)]">
      {t('settings.adapters.status.empty')}
    </span>
  )
}

function NumberField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string
  hint?: string
  value: number | undefined
  onChange: (v: number | undefined) => void
}) {
  return (
    <div>
      <label className="block text-xs font-medium text-[var(--color-text-primary)] mb-1">
        {label}
      </label>
      <input
        type="number"
        value={typeof value === 'number' ? value : ''}
        onChange={(e) => {
          const raw = e.target.value
          if (raw === '') onChange(undefined)
          else {
            const n = Number(raw)
            onChange(Number.isFinite(n) ? n : undefined)
          }
        }}
        className="h-8 w-full px-2.5 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-xs text-[var(--color-text-primary)] focus:border-[var(--color-border-focus)] focus:shadow-[var(--shadow-focus-ring)] outline-none"
      />
      {hint && <p className="mt-1 text-xs text-[var(--color-text-tertiary)]">{hint}</p>}
    </div>
  )
}

function SelectField({
  label,
  value,
  options,
  onChange,
}: {
  label: string
  value: string
  options: Array<{ value: string; label: string }>
  onChange: (v: string) => void
}) {
  return (
    <div>
      <label className="block text-xs font-medium text-[var(--color-text-primary)] mb-1">
        {label}
      </label>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="h-8 w-full px-2.5 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-xs text-[var(--color-text-primary)] focus:border-[var(--color-border-focus)] focus:shadow-[var(--shadow-focus-ring)] outline-none cursor-pointer"
      >
        <option value=""></option>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </div>
  )
}

function CheckboxField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string
  hint?: string
  value: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <label className="flex items-start gap-2 cursor-pointer py-1.5">
      <input
        type="checkbox"
        checked={value}
        onChange={(e) => onChange(e.target.checked)}
        className="mt-0.5 w-4 h-4 rounded border-[var(--color-border)] accent-[var(--color-brand)]"
      />
      <div className="flex-1 min-w-0">
        <span className="text-xs text-[var(--color-text-primary)]">{label}</span>
        {hint && <p className="mt-0.5 text-xs text-[var(--color-text-tertiary)]">{hint}</p>}
      </div>
    </label>
  )
}

function stripUndefined<T extends Record<string, unknown>>(obj: T): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(obj)) {
    if (v === undefined) continue
    out[k] = v
  }
  return out
}
