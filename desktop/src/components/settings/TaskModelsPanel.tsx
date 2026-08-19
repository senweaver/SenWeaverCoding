// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { agentSettingsApi } from '../../api/agentSettings'
import { useTranslation } from '../../i18n'
import type { SavedProvider } from '../../types/provider'
import type { ProviderPreset } from '../../types/providerPreset'
import { aggregateProviderModels } from '../../utils/providerModels'
import {
  SettingsSection,
  SettingsSectionStatus,
  type SettingsSectionStatusValue,
} from './SettingsSection'

export function TaskModelsPanel({
  providers,
  presetMap,
}: {
  providers: SavedProvider[]
  presetMap: Map<string, ProviderPreset>
}) {
  const t = useTranslation()
  const [loaded, setLoaded] = useState(false)
  const [value, setValue] = useState('')
  const [saving, setSaving] = useState(false)
  const [status, setStatus] = useState<SettingsSectionStatusValue>(null)

  useEffect(() => {
    let cancelled = false
    agentSettingsApi
      .getAgentRuntime()
      .then((cfg) => {
        if (cancelled) return
        setValue(cfg.subagentModel ?? '')
        setLoaded(true)
      })
      .catch((err) => {
        if (cancelled) return
        setStatus({
          kind: 'error',
          text: err instanceof Error ? err.message : String(err),
        })
      })
    return () => {
      cancelled = true
    }
  }, [])

  const modelOptions = useMemo(() => {
    const seen = new Set<string>()
    const out: Array<{ value: string; label: string }> = []
    for (const entry of aggregateProviderModels(providers, presetMap)) {
      if (!entry.enabled || seen.has(entry.modelId)) continue
      seen.add(entry.modelId)
      out.push({
        value: entry.modelId,
        label: `${entry.modelId} · ${entry.providerName}`,
      })
    }
    return out
  }, [providers, presetMap])

  const hasUnknownValue = value !== '' && !modelOptions.some((opt) => opt.value === value)

  async function handleChange(next: string) {
    const previous = value
    setValue(next)
    setSaving(true)
    setStatus(null)
    try {
      await agentSettingsApi.updateAgentRuntime({
        subagentModel: next.trim() ? next.trim() : null,
      })
      setStatus({ kind: 'ok', text: t('settings.taskModels.saved') })
    } catch (err) {
      setValue(previous)
      setStatus({
        kind: 'error',
        text: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setSaving(false)
    }
  }

  return (
    <SettingsSection
      title={t('settings.taskModels.title')}
      description={t('settings.taskModels.description')}
    >
      <div className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] px-3 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium text-[var(--color-text-primary)]">
            {t('settings.taskModels.subagentLabel')}
          </div>
          <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
            {t('settings.taskModels.subagentHint')}
          </div>
        </div>
        <select
          value={value}
          disabled={!loaded || saving}
          onChange={(e) => void handleChange(e.target.value)}
          className="h-8 w-[260px] shrink-0 px-2.5 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50 cursor-pointer"
        >
          <option value="">{t('settings.taskModels.inherit')}</option>
          {hasUnknownValue && <option value={value}>{value}</option>}
          {modelOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </div>
      <div className="flex items-center min-h-[16px]">
        <SettingsSectionStatus status={status} />
      </div>
    </SettingsSection>
  )
}
