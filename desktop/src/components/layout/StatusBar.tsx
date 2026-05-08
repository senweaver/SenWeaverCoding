import { useEffect, useMemo, useState } from 'react'
import { useSettingsStore } from '../../stores/settingsStore'
import { useSessionRuntimeStore, DRAFT_RUNTIME_SELECTION_KEY } from '../../stores/sessionRuntimeStore'
import { useProviderStore } from '../../stores/providerStore'
import { useTabStore } from '../../stores/tabStore'
import type { CodingModeId } from '../../types/codingMode'
import { useTranslation, useCodingModeText } from '../../i18n'

const STATUS_MODE_GLYPH: Record<CodingModeId, string> = {
  vibe: 'bolt',
  agent: 'robot_2',
  spec: 'description',
  plan: 'architecture',
  ask: 'help',
  tdd: 'science',
  debug: 'bug_report',
  architect: 'design_services',
  pair: 'group',
  context: 'data_object',
  mvai: 'hub',
  harness: 'precision_manufacturing',
}

const STATUS_AUTONOMOUS_MODES = new Set<CodingModeId>(['agent', 'harness'])
const STATUS_READONLY_MODES = new Set<CodingModeId>(['ask'])

export function StatusBar() {
  const t = useTranslation()
  const tCodingMode = useCodingModeText()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const providers = useProviderStore((s) => s.providers)

  const runtimeSelection = useSessionRuntimeStore((s) =>
    activeTabId
      ? s.selections[activeTabId] ?? s.selections[DRAFT_RUNTIME_SELECTION_KEY]
      : s.selections[DRAFT_RUNTIME_SELECTION_KEY],
  )

  const codingMode = useSettingsStore((s) => s.codingMode)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const settingsModel = useSettingsStore((s) => s.currentModel)
  const settingsProviderName = useSettingsStore((s) => s.activeProviderName)

  const codingModeBackendLabel = codingModes.find((m) => m.id === codingMode)?.label
  const codingModeLabel = tCodingMode(
    codingMode,
    'label',
    codingModeBackendLabel ?? codingMode.charAt(0).toUpperCase() + codingMode.slice(1),
  )

  const codingModeGlyph = STATUS_MODE_GLYPH[codingMode] ?? 'tune'
  const codingModeIsAutonomous = STATUS_AUTONOMOUS_MODES.has(codingMode)
  const codingModeIsReadonly = STATUS_READONLY_MODES.has(codingMode)
  const codingModeIsPlan = codingMode === 'plan'

  const codingModeBadgeClass = codingModeIsAutonomous
    ? 'flex items-center gap-1 rounded-full bg-[var(--color-error)]/12 px-1.5 py-px text-[var(--color-error)]'
    : codingModeIsReadonly
    ? 'flex items-center gap-1 rounded-full bg-[var(--color-surface-container)] px-1.5 py-px text-[var(--color-text-tertiary)]'
    : codingModeIsPlan
    ? 'flex items-center gap-1 rounded-full bg-[var(--color-plan-accent-container)] px-1.5 py-px text-[var(--color-on-plan-accent-container)]'
    : 'flex items-center gap-1'

  const codingModeIconClass = codingModeIsAutonomous
    ? 'material-symbols-outlined text-[11px] text-[var(--color-error)]'
    : codingModeIsReadonly
    ? 'material-symbols-outlined text-[11px] text-[var(--color-text-tertiary)]'
    : codingModeIsPlan
    ? 'material-symbols-outlined text-[11px] text-[var(--color-on-plan-accent-container)]'
    : 'material-symbols-outlined text-[11px] text-[var(--color-text-tertiary)]'

  const codingModeTitle = codingModeIsAutonomous
    ? `${codingModeLabel} · ${t('codingMode.tag.autonomous')}`
    : codingModeIsReadonly
    ? `${codingModeLabel} · ${t('codingMode.tag.readOnly')}`
    : codingModeLabel

  const { providerLabel, modelLabel } = useMemo(() => {
    if (runtimeSelection) {
      const provider = providers.find((p) => p.id === runtimeSelection.providerId)
      return {
        providerLabel: provider?.name ?? null,
        modelLabel: runtimeSelection.modelId,
      }
    }
    if (settingsModel) {
      return {
        providerLabel: settingsProviderName ?? null,
        modelLabel: settingsModel.name ?? settingsModel.id,
      }
    }
    return { providerLabel: null as string | null, modelLabel: null as string | null }
  }, [providers, runtimeSelection, settingsModel, settingsProviderName])

  const [version, setVersion] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    import('@tauri-apps/api/app')
      .then((mod) => mod.getVersion())
      .then((value) => { if (!cancelled) setVersion(value) })
      .catch(() => { if (!cancelled) setVersion(null) })
    return () => { cancelled = true }
  }, [])

  return (
    <div
      role="status"
      aria-label="Status bar"
      className="flex h-[22px] flex-shrink-0 items-center justify-between border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 text-[11px] text-[var(--color-text-secondary)]"
    >
      {}
      <div className="flex items-center gap-3">
        <span className={codingModeBadgeClass} title={codingModeTitle}>
          <span className={codingModeIconClass}>{codingModeGlyph}</span>
          <span>{codingModeLabel}</span>
          {codingModeIsAutonomous && (
            <span className="text-[9px] uppercase tracking-wider font-bold">
              {t('codingMode.tag.autonomous')}
            </span>
          )}
          {codingModeIsReadonly && (
            <span className="text-[9px] uppercase tracking-wider font-bold">
              {t('codingMode.tag.readOnly')}
            </span>
          )}
        </span>

        {modelLabel && (
          <span className="flex items-center gap-1">
            <span className="text-[var(--color-text-tertiary)]">·</span>
            <span
              className="truncate"
              title={providerLabel ? `${providerLabel} · ${modelLabel}` : modelLabel}
            >
              {modelLabel}
              {providerLabel && (
                <span className="text-[var(--color-text-tertiary)]"> · {providerLabel}</span>
              )}
            </span>
          </span>
        )}
      </div>

      {}
      <span className="text-[var(--color-text-tertiary)]">
        {version ? `v${version}` : ''}
      </span>
    </div>
  )
}
