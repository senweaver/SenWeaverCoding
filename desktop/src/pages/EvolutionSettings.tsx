import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../i18n'
import { Button } from '../components/shared/Button'
import { Modal } from '../components/shared/Modal'
import { Input } from '../components/shared/Input'
import { ConfirmDialog } from '../components/shared/ConfirmDialog'
import { useEvolutionStore } from '../stores/evolutionStore'
import { useUIStore } from '../stores/uiStore'
import type { TranslationKey } from '../i18n'
import type {
  AvailableModelEntry,
  AvailableProviderEntry,
  CloudTarget,
  EvolutionExportFormatId,
  EvolutionOverview,
  ExperienceRecyclingConfig,
  PurgeScopeId,
  RecycledExperienceItem,
  ReflectionDepthId,
  ReflectionRunItem,
  ReflectionRunStatusId,
  ReflectionSummary,
  ReflectionTriggerModeId,
  ReflectionWritebackTargetId,
  SelfReflectionConfig,
} from '../types/evolution'

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unitIdx = 0
  while (value >= 1024 && unitIdx < units.length - 1) {
    value /= 1024
    unitIdx += 1
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unitIdx]}`
}

function formatNumber(n: number | undefined | null): string {
  if (typeof n !== 'number' || !Number.isFinite(n)) return '0'
  return Math.round(n).toLocaleString()
}

function formatTimestamp(value: string | null): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString()
  } catch {
    return value
  }
}

function formatRelativeTime(value: string | null): string {
  if (!value) return '—'
  const ms = Date.parse(value)
  if (Number.isNaN(ms)) return value
  const diff = Date.now() - ms
  const seconds = Math.round(diff / 1000)
  if (seconds < 0) {
    const future = Math.abs(seconds)
    if (future < 60) return `in ${future}s`
    if (future < 3600) return `in ${Math.round(future / 60)}m`
    if (future < 86400) return `in ${Math.round(future / 3600)}h`
    return `in ${Math.round(future / 86400)}d`
  }
  if (seconds < 60) return `${seconds}s ago`
  if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`
  if (seconds < 86400) return `${Math.round(seconds / 3600)}h ago`
  return `${Math.round(seconds / 86400)}d ago`
}

function statusBadgeClass(status: ReflectionRunStatusId | string): string {
  if (status === 'completed') {
    return 'bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
  }
  if (status === 'failed') {
    return 'bg-[var(--color-error)]/15 text-[var(--color-error)]'
  }
  if (status === 'running') {
    return 'bg-[var(--color-warning)]/15 text-[var(--color-warning)]'
  }
  if (status === 'queued') {
    return 'bg-[var(--color-info,#3b82f6)]/15 text-[var(--color-info,#3b82f6)]'
  }
  return 'bg-[var(--color-surface-hover)] text-[var(--color-text-tertiary)]'
}

export function EvolutionSettings() {
  const t = useTranslation()
  const overview = useEvolutionStore((s) => s.overview)
  const config = useEvolutionStore((s) => s.config)
  const lessons = useEvolutionStore((s) => s.lessons)
  const persistence = useEvolutionStore((s) => s.persistence)
  const exportFormats = useEvolutionStore((s) => s.exportFormats)
  const exports = useEvolutionStore((s) => s.exports)
  const cloudTargets = useEvolutionStore((s) => s.cloudTargets)
  const pushHistory = useEvolutionStore((s) => s.pushHistory)
  const recyclingConfig = useEvolutionStore((s) => s.recyclingConfig)
  const recyclingItems = useEvolutionStore((s) => s.recyclingItems)
  const recyclingTotal = useEvolutionStore((s) => s.recyclingTotal)
  const reflectionConfig = useEvolutionStore((s) => s.reflectionConfig)
  const reflectionRuns = useEvolutionStore((s) => s.reflectionRuns)
  const reflectionSummary = useEvolutionStore((s) => s.reflectionSummary)
  const availableModels = useEvolutionStore((s) => s.availableModels)
  const availableProviders = useEvolutionStore((s) => s.availableProviders)
  const availableModelsProvidersConfigured = useEvolutionStore(
    (s) => s.availableModelsProvidersConfigured,
  )
  const fetchAll = useEvolutionStore((s) => s.fetchAll)
  const lastPersistAutoEnabledAt = useEvolutionStore(
    (s) => s.lastPersistAutoEnabledAt,
  )
  const addToast = useUIStore((s) => s.addToast)
  const lastNotifiedRef = useRef<number | null>(null)

  useEffect(() => {
    void fetchAll()
  }, [fetchAll])

  useEffect(() => {
    if (
      lastPersistAutoEnabledAt &&
      lastPersistAutoEnabledAt !== lastNotifiedRef.current
    ) {
      lastNotifiedRef.current = lastPersistAutoEnabledAt
      addToast({
        type: 'info',
        message: t('settings.evolution.reflection.persistAutoEnabledToast'),
      })
    }
  }, [lastPersistAutoEnabledAt, addToast, t])

  return (
    <div className="max-w-4xl flex flex-col gap-6">
      <div>
        <h2 className="text-base font-semibold text-[var(--color-text-primary)] mb-1">
          {t('settings.evolution.title')}
        </h2>
        <p className="text-xs text-[var(--color-text-tertiary)]">
          {t('settings.evolution.description')}
        </p>
      </div>

      <PersistenceCard persistence={persistence} configEnabled={config?.persistTrainingData ?? false} />
      <MetricsCard overview={overview} />
      <RecyclingCard
        config={recyclingConfig}
        items={recyclingItems}
        total={recyclingTotal}
        overview={overview}
        persistEnabled={config?.persistTrainingData ?? false}
      />
      <ReflectionCard
        config={reflectionConfig}
        runs={reflectionRuns}
        summary={reflectionSummary}
        availableModels={availableModels}
        availableProviders={availableProviders}
        providersConfigured={availableModelsProvidersConfigured}
        persistEnabled={config?.persistTrainingData ?? false}
      />
      <MaintenanceCard persistEnabled={config?.persistTrainingData ?? false} />
      <EngineConfigCard
        config={config}
        availableModels={availableModels}
        providersConfigured={availableModelsProvidersConfigured}
      />
      <LessonsCard lessons={lessons} />
      <ExportCard
        persistEnabled={config?.persistTrainingData ?? false}
        formats={exportFormats}
        exports={exports}
      />
      <CloudTargetsCard
        persistEnabled={config?.persistTrainingData ?? false}
        targets={cloudTargets}
        exports={exports}
        pushHistory={pushHistory}
      />
    </div>
  )
}

function ModelPicker({
  label,
  value,
  models,
  providersConfigured,
  onChange,
  allowEmpty = true,
  helperText,
}: {
  label: string
  value: string | null
  models: AvailableModelEntry[]
  providersConfigured: number
  onChange: (next: string | null) => void
  allowEmpty?: boolean
  helperText?: string
}) {
  const t = useTranslation()
  const noModels = models.length === 0
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-medium text-[var(--color-text-secondary)]">{label}</span>
        {noModels && (
          <span className="text-[10px] uppercase tracking-wider text-[var(--color-error)]">
            {t('settings.evolution.modelPicker.noModelsConfigured')}
          </span>
        )}
      </div>
      <select
        value={value ?? ''}
        disabled={noModels}
        onChange={(e) => {
          const next = e.target.value
          onChange(next === '' ? null : next)
        }}
        className="w-full text-xs rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-2 py-1.5 text-[var(--color-text-primary)] disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {allowEmpty && (
          <option value="">
            {noModels
              ? providersConfigured > 0
                ? t('settings.evolution.modelPicker.providerHasNoModels')
                : t('settings.evolution.modelPicker.addProviderFirst')
              : t('settings.evolution.modelPicker.useDefault')}
          </option>
        )}
        {value && !models.some((m) => m.id === value) && (
          <option value={value}>
            {t('settings.evolution.modelPicker.unregisteredHint', { model: value })}
          </option>
        )}
        {models.map((m) => (
          <option key={`${m.providerId}::${m.id}`} value={m.id}>
            {m.id}
            {m.providerName ? ` · ${m.providerName}` : ''}
          </option>
        ))}
      </select>
      {helperText && (
        <span className="text-[11px] text-[var(--color-text-tertiary)]">{helperText}</span>
      )}
      {noModels && (
        <span className="text-[11px] text-[var(--color-text-tertiary)]">
          {t('settings.evolution.modelPicker.goToAdapterSettings')}
        </span>
      )}
    </div>
  )
}

function ProviderPicker({
  label,
  value,
  providers,
  onChange,
}: {
  label: string
  value: string | null
  providers: AvailableProviderEntry[]
  onChange: (next: string | null) => void
}) {
  const t = useTranslation()
  const noProviders = providers.length === 0
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs font-medium text-[var(--color-text-secondary)]">{label}</span>
      <select
        value={value ?? ''}
        disabled={noProviders}
        onChange={(e) => {
          const next = e.target.value
          onChange(next === '' ? null : next)
        }}
        className="w-full text-xs rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-2 py-1.5 text-[var(--color-text-primary)] disabled:opacity-50 disabled:cursor-not-allowed"
      >
        <option value="">{t('settings.evolution.modelPicker.providerAuto')}</option>
        {providers.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
            {p.isDefault ? ` · ${t('settings.evolution.modelPicker.providerDefaultTag')}` : ''}
          </option>
        ))}
      </select>
    </div>
  )
}

function SectionShell({
  title,
  description,
  children,
  rightAction,
}: {
  title: string
  description?: string
  rightAction?: React.ReactNode
  children: React.ReactNode
}) {
  return (
    <section className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] p-4">
      <div className="flex items-start justify-between mb-3">
        <div>
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{title}</h3>
          {description && (
            <p className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{description}</p>
          )}
        </div>
        {rightAction}
      </div>
      {children}
    </section>
  )
}

function PersistenceCard({
  persistence,
  configEnabled,
}: {
  persistence: ReturnType<typeof useEvolutionStore.getState>['persistence']
  configEnabled: boolean
}) {
  const t = useTranslation()
  const setPersistence = useEvolutionStore((s) => s.setPersistence)
  const purge = useEvolutionStore((s) => s.purge)
  const [purgeOpen, setPurgeOpen] = useState<PurgeScopeId | null>(null)
  const [confirmText, setConfirmText] = useState('')
  const [busy, setBusy] = useState(false)
  const enabled = persistence?.persistTrainingData ?? configEnabled

  const handleToggle = async (next: boolean) => {
    setBusy(true)
    try {
      await setPersistence(next)
    } finally {
      setBusy(false)
    }
  }

  const handlePurge = async () => {
    if (!purgeOpen) return
    if (confirmText !== 'I_UNDERSTAND') return
    setBusy(true)
    try {
      await purge(purgeOpen, null)
    } finally {
      setPurgeOpen(null)
      setConfirmText('')
      setBusy(false)
    }
  }

  return (
    <SectionShell
      title={t('settings.evolution.persistence.title')}
      description={t('settings.evolution.persistence.description')}
      rightAction={
        <button
          onClick={() => void handleToggle(!enabled)}
          disabled={busy}
          className={`whitespace-nowrap text-xs font-semibold px-3 py-1.5 rounded-lg border transition-all ${
            enabled
              ? 'border-[var(--color-brand)] bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
              : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
          }`}
        >
          {enabled
            ? t('settings.evolution.persistence.toggleOn')
            : t('settings.evolution.persistence.toggleOff')}
        </button>
      }
    >
      <div className="grid grid-cols-3 gap-3 text-xs mb-4">
        <Stat label={t('settings.evolution.metrics.totalTurns')} value={formatNumber(persistence?.turnsCount)} />
        <Stat label={t('settings.evolution.persistence.usage')} value={formatBytes((persistence?.turnsFileSize ?? 0) + (persistence?.eventsFileSize ?? 0))} />
        <Stat label={t('settings.evolution.metrics.exports')} value={formatBytes(persistence?.exportsTotalBytes ?? 0)} />
      </div>
      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" size="sm" onClick={() => setPurgeOpen('turns')}>
          {t('settings.evolution.persistence.purgeTurns')}
        </Button>
        <Button variant="secondary" size="sm" onClick={() => setPurgeOpen('exports')}>
          {t('settings.evolution.persistence.purgeExports')}
        </Button>
        <Button variant="secondary" size="sm" onClick={() => setPurgeOpen('push_history')}>
          {t('settings.evolution.persistence.purgeHistory')}
        </Button>
        <Button variant="secondary" size="sm" onClick={() => setPurgeOpen('events')}>
          {t('settings.evolution.persistence.purgeEvents')}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => setPurgeOpen('all')}
          className="text-[var(--color-error)]"
        >
          {t('settings.evolution.persistence.purgeAll')}
        </Button>
      </div>

      <Modal
        open={purgeOpen !== null}
        onClose={() => {
          if (busy) return
          setPurgeOpen(null)
          setConfirmText('')
        }}
        title={t('settings.evolution.persistence.purgeTitle')}
        width={420}
        footer={
          <>
            <Button variant="secondary" onClick={() => { setPurgeOpen(null); setConfirmText('') }}>
              Cancel
            </Button>
            <Button
              onClick={() => void handlePurge()}
              disabled={confirmText !== 'I_UNDERSTAND' || busy}
              loading={busy}
            >
              Confirm
            </Button>
          </>
        }
      >
        <div className="text-xs text-[var(--color-text-secondary)] mb-3">
          {t('settings.evolution.persistence.purgeWarning')}
        </div>
        <Input value={confirmText} onChange={(e) => setConfirmText(e.target.value)} placeholder="I_UNDERSTAND" />
      </Modal>
    </SectionShell>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-2">
      <div className="text-[10px] uppercase tracking-wider text-[var(--color-text-tertiary)]">{label}</div>
      <div className="text-base font-semibold text-[var(--color-text-primary)]">{value}</div>
    </div>
  )
}

function MetricsCard({
  overview,
}: {
  overview: EvolutionOverview | null
}) {
  const t = useTranslation()
  const judge = overview?.judgeWorker
  const scheduler = overview?.reflectionScheduler
  const recycling = overview?.recycling
  const tools = overview?.tools
  const judgeState: 'running' | 'idle' | 'error' = judge?.lastErrorAt
    ? 'error'
    : judge?.running
      ? 'running'
      : 'idle'
  const judgeKey =
    judgeState === 'running'
      ? 'settings.evolution.overview.judgeWorker.running'
      : judgeState === 'error'
        ? 'settings.evolution.overview.judgeWorker.error'
        : 'settings.evolution.overview.judgeWorker.idle'
  const judgeClass =
    judgeState === 'running'
      ? 'bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
      : judgeState === 'error'
        ? 'bg-[var(--color-error)]/15 text-[var(--color-error)]'
        : 'bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]'
  return (
    <SectionShell title={t('settings.evolution.metrics.title')}>
      <div className="grid grid-cols-3 gap-3 text-xs">
        <Stat label={t('settings.evolution.metrics.totalTurns')} value={formatNumber(overview?.totalTurns)} />
        <Stat label={t('settings.evolution.metrics.lessons')} value={formatNumber(overview?.lessonsTotal)} />
        <Stat label={t('settings.evolution.metrics.lessonsActive')} value={formatNumber(overview?.lessonsActive)} />
        <Stat label={t('settings.evolution.metrics.lessonHits')} value={formatNumber(overview?.lessonHitsTotal)} />
        <Stat label={t('settings.evolution.metrics.exports')} value={formatNumber(overview?.exportsCount)} />
        <Stat label={t('settings.evolution.metrics.pushes')} value={formatNumber(overview?.pushReceiptsCount)} />
      </div>
      {(judge || scheduler || recycling || tools) && (
        <div className="flex flex-wrap gap-2 mt-3">
          {judge && (
            <span
              className={`text-[10px] uppercase tracking-wider px-2 py-0.5 rounded ${judgeClass}`}
              title={judge.lastErrorMessage ?? undefined}
            >
              {t(judgeKey as TranslationKey)}
              {' · '}
              {formatNumber(judge.processed)}/{formatNumber(judge.enqueuedTotal)}
            </span>
          )}
          {scheduler && (
            <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]">
              {t('settings.evolution.overview.reflectionScheduler.label')}
              {' · '}
              {t('settings.evolution.overview.reflectionScheduler.intervalMinutes', {
                minutes: String(scheduler.intervalMinutes),
              })}
              {' · '}
              {scheduler.lastTickAt
                ? formatRelativeTime(scheduler.lastTickAt)
                : t('settings.evolution.overview.reflectionScheduler.never')}
            </span>
          )}
          {recycling && (
            <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]">
              {t('settings.evolution.recycling.runtime.todayHarvested', {
                count: String(recycling.recent24hHarvested),
              })}
              {' · '}
              {t('settings.evolution.recycling.runtime.totalHarvested', {
                count: String(recycling.totalHarvested),
              })}
            </span>
          )}
          {tools && (
            <span
              className="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]"
              title={t('settings.evolution.metrics.toolSearchTooltip')}
            >
              {t('settings.evolution.metrics.toolSearch')}
              {' · '}
              {formatNumber(tools.invocations)} / {formatNumber(tools.activations)} / {formatNumber(tools.highRiskBlocked)}
              {' · '}
              {tools.avgLatencyMs.toFixed(1)} ms
              {typeof tools.deferredBuiltinCount === 'number' && tools.deferredBuiltinCount > 0 && (
                <>
                  {' · '}
                  {t('settings.evolution.metrics.deferredBuiltin', {
                    count: String(tools.deferredBuiltinCount),
                  })}
                </>
              )}
            </span>
          )}
        </div>
      )}
    </SectionShell>
  )
}

function MaintenanceCard({ persistEnabled }: { persistEnabled: boolean }) {
  const t = useTranslation()
  const distillTurn = useEvolutionStore((s) => s.distillTurn)
  const rescoreAll = useEvolutionStore((s) => s.rescoreAll)
  const [turnId, setTurnId] = useState('')
  const [busy, setBusy] = useState<'distill' | 'rescore' | null>(null)
  const [feedback, setFeedback] = useState<string | null>(null)

  const handleDistill = async () => {
    const trimmed = turnId.trim()
    if (!trimmed) return
    setBusy('distill')
    setFeedback(null)
    try {
      const result = await distillTurn(trimmed)
      if (result?.queued) {
        setFeedback(
          t('settings.evolution.maintenance.distillQueued', { turnId: result.turnId }),
        )
        setTurnId('')
      } else {
        setFeedback(t('settings.evolution.maintenance.distillFailed'))
      }
    } finally {
      setBusy(null)
    }
  }

  const handleRescore = async () => {
    setBusy('rescore')
    setFeedback(null)
    try {
      const result = await rescoreAll()
      if (result) {
        setFeedback(
          t('settings.evolution.maintenance.rescoreDone', {
            rescored: String(result.rescored),
            errors: String(result.errors),
            totalSeen: String(result.totalSeen),
          }),
        )
      } else {
        setFeedback(t('settings.evolution.maintenance.rescoreFailed'))
      }
    } finally {
      setBusy(null)
    }
  }

  return (
    <SectionShell
      title={t('settings.evolution.maintenance.title')}
      description={t('settings.evolution.maintenance.description')}
    >
      {!persistEnabled && (
        <p className="text-xs text-[var(--color-text-tertiary)] mb-3">
          {t('settings.evolution.maintenance.persistRequired')}
        </p>
      )}
      <div className="flex flex-col gap-3">
        <div className="flex items-end gap-2">
          <div className="flex-1">
            <Input
              label={t('settings.evolution.maintenance.turnIdLabel')}
              value={turnId}
              onChange={(e) => setTurnId(e.target.value)}
              placeholder="turn_..."
            />
          </div>
          <Button
            onClick={() => void handleDistill()}
            disabled={!turnId.trim() || !persistEnabled || busy !== null}
            loading={busy === 'distill'}
          >
            {t('settings.evolution.maintenance.distill')}
          </Button>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-xs text-[var(--color-text-secondary)]">
            {t('settings.evolution.maintenance.rescoreHint')}
          </span>
          <Button
            variant="secondary"
            onClick={() => void handleRescore()}
            disabled={!persistEnabled || busy !== null}
            loading={busy === 'rescore'}
          >
            {t('settings.evolution.maintenance.rescore')}
          </Button>
        </div>
        {feedback && (
          <div className="text-xs text-[var(--color-text-secondary)] bg-[var(--color-surface-container)] rounded-md px-2 py-1">
            {feedback}
          </div>
        )}
      </div>
    </SectionShell>
  )
}

function EngineConfigCard({
  config,
  availableModels,
  providersConfigured,
}: {
  config: ReturnType<typeof useEvolutionStore.getState>['config']
  availableModels: AvailableModelEntry[]
  providersConfigured: number
}) {
  const t = useTranslation()
  const updateConfig = useEvolutionStore((s) => s.updateConfig)
  if (!config) return null
  return (
    <SectionShell title={t('settings.evolution.config.title')}>
      <div className="flex flex-col gap-3">
        <ToggleRow
          label={t('settings.evolution.config.judgeEnabled')}
          value={config.nextStateJudgeEnabled}
          onChange={(next) => void updateConfig({ nextStateJudgeEnabled: next })}
        />
        <ToggleRow
          label={t('settings.evolution.config.autoDistill')}
          value={config.autoDistillOnSessionEnd}
          onChange={(next) => void updateConfig({ autoDistillOnSessionEnd: next })}
        />
        <NumberRow
          label={t('settings.evolution.config.maxLessons')}
          value={config.maxLessonsInPrompt}
          onChange={(next) => void updateConfig({ maxLessonsInPrompt: next })}
          min={0}
          max={20}
        />
        <NumberRow
          label={t('settings.evolution.config.tokenBudget')}
          value={config.lessonTokenBudget}
          onChange={(next) => void updateConfig({ lessonTokenBudget: next })}
          min={64}
          max={8000}
          step={64}
        />
        <ModelPicker
          label={t('settings.evolution.config.judgeModel')}
          value={config.judgeModel}
          models={availableModels}
          providersConfigured={providersConfigured}
          onChange={(next) => {
            void updateConfig({ judgeModel: next }).catch((error: unknown) => {
              console.warn('[evolution] judge model update failed', error)
            })
          }}
        />
      </div>
    </SectionShell>
  )
}

function ToggleRow({
  label,
  value,
  onChange,
}: {
  label: string
  value: boolean
  onChange: (next: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between text-xs">
      <span className="text-[var(--color-text-primary)]">{label}</span>
      <button
        onClick={() => onChange(!value)}
        className={`relative inline-flex h-5 w-9 rounded-full border transition-all ${
          value
            ? 'bg-[var(--color-brand)] border-[var(--color-brand)]'
            : 'bg-[var(--color-surface-container)] border-[var(--color-border)]'
        }`}
      >
        <span
          className={`absolute top-0.5 ${value ? 'left-4' : 'left-0.5'} w-4 h-4 rounded-full bg-white shadow transition-all`}
        />
      </button>
    </div>
  )
}

function NumberRow({
  label,
  value,
  onChange,
  min,
  max,
  step,
}: {
  label: string
  value: number
  onChange: (next: number) => void
  min?: number
  max?: number
  step?: number
}) {
  return (
    <div className="flex items-center justify-between text-xs gap-3">
      <span className="text-[var(--color-text-primary)] flex-1">{label}</span>
      <input
        type="number"
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        min={min}
        max={max}
        step={step ?? 1}
        className="w-32 text-xs px-2 py-1 rounded-md bg-[var(--color-surface-container)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
      />
    </div>
  )
}

function LessonsCard({
  lessons,
}: {
  lessons: ReturnType<typeof useEvolutionStore.getState>['lessons']
}) {
  const t = useTranslation()
  const updateLesson = useEvolutionStore((s) => s.updateLesson)
  const deleteLesson = useEvolutionStore((s) => s.deleteLesson)
  return (
    <SectionShell title={t('settings.evolution.lessons.title')}>
      {lessons.length === 0 ? (
        <div className="text-xs text-[var(--color-text-tertiary)]">{t('settings.evolution.lessons.empty')}</div>
      ) : (
        <div className="flex flex-col gap-2">
          {lessons.map((l) => (
            <div
              key={l.id}
              className="flex items-start justify-between gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-2"
            >
              <div className="flex-1 min-w-0">
                <div className="text-xs font-semibold text-[var(--color-text-primary)] truncate">
                  {l.title}
                </div>
                <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5 line-clamp-2">{l.body}</div>
                <div className="text-[10px] text-[var(--color-text-tertiary)] mt-1">
                  {l.codingMode ?? 'global'} · hits {l.hits}
                  {l.tags.length > 0 && ` · ${l.tags.join(', ')}`}
                </div>
              </div>
              <div className="flex items-center gap-2 flex-shrink-0">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void updateLesson(l.id, { enabled: !l.enabled })}
                >
                  {l.enabled ? 'disable' : 'enable'}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-[var(--color-error)]"
                  onClick={() => void deleteLesson(l.id)}
                >
                  delete
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}
    </SectionShell>
  )
}

function ExportCard({
  persistEnabled,
  formats,
  exports,
}: {
  persistEnabled: boolean
  formats: Array<{ id: EvolutionExportFormatId; label: string }>
  exports: ReturnType<typeof useEvolutionStore.getState>['exports']
}) {
  const t = useTranslation()
  const createExport = useEvolutionStore((s) => s.createExport)
  const deleteExport = useEvolutionStore((s) => s.deleteExport)
  const [format, setFormat] = useState<EvolutionExportFormatId>(formats[0]?.id ?? 'openai_sft')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (!format && formats[0]) setFormat(formats[0].id)
  }, [format, formats])

  const handleCreate = async () => {
    setBusy(true)
    try {
      await createExport(format)
    } finally {
      setBusy(false)
    }
  }

  return (
    <SectionShell
      title={t('settings.evolution.export.title')}
      description={t('settings.evolution.export.description')}
    >
      {!persistEnabled && (
        <div className="text-xs text-[var(--color-warning)] mb-3 px-2 py-1.5 rounded-md bg-[var(--color-warning)]/15 border border-[var(--color-warning)]/30">
          {t('settings.evolution.export.disabled')}
        </div>
      )}
      <div className="flex items-center gap-2 mb-4">
        <select
          value={format}
          onChange={(e) => setFormat(e.target.value as EvolutionExportFormatId)}
          disabled={!persistEnabled || formats.length === 0}
          className="text-xs px-3 py-1.5 rounded-md bg-[var(--color-surface-container)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none"
        >
          {formats.map((f) => (
            <option key={f.id} value={f.id}>
              {f.label}
            </option>
          ))}
        </select>
        <Button onClick={() => void handleCreate()} loading={busy} disabled={!persistEnabled}>
          {t('settings.evolution.export.create')}
        </Button>
      </div>
      <div>
        <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)] mb-2">
          {t('settings.evolution.export.recent')}
        </div>
        {exports.length === 0 ? (
          <div className="text-xs text-[var(--color-text-tertiary)]">{t('settings.evolution.export.empty')}</div>
        ) : (
          <div className="flex flex-col gap-1">
            {exports.slice(0, 8).map((e) => (
              <div
                key={e.id}
                className="flex items-center justify-between gap-3 text-xs rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-1.5"
              >
                <div className="flex-1 min-w-0">
                  <div className="font-semibold text-[var(--color-text-primary)] truncate">
                    {e.format} · {formatNumber(e.sampleCount)} samples · {formatBytes(e.sizeBytes)}
                  </div>
                  <div className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                    {formatTimestamp(e.createdAt)} · {e.path}
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-[var(--color-error)]"
                  onClick={() => void deleteExport(e.id)}
                >
                  delete
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>
    </SectionShell>
  )
}

function CloudTargetsCard({
  persistEnabled,
  targets,
  exports,
  pushHistory,
}: {
  persistEnabled: boolean
  targets: CloudTarget[]
  exports: ReturnType<typeof useEvolutionStore.getState>['exports']
  pushHistory: ReturnType<typeof useEvolutionStore.getState>['pushHistory']
}) {
  const t = useTranslation()
  const upsert = useEvolutionStore((s) => s.upsertCloudTarget)
  const remove = useEvolutionStore((s) => s.deleteCloudTarget)
  const push = useEvolutionStore((s) => s.push)
  const [editing, setEditing] = useState<CloudTarget | null>(null)
  const [showAdd, setShowAdd] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<CloudTarget | null>(null)
  const [pushPickerForTarget, setPushPickerForTarget] = useState<CloudTarget | null>(null)
  const [pushExportId, setPushExportId] = useState<string>('')

  const targetMap = useMemo(() => new Map(targets.map((t) => [t.id, t])), [targets])

  return (
    <SectionShell
      title={t('settings.evolution.cloud.title')}
      description={t('settings.evolution.cloud.description')}
      rightAction={
        <Button
          size="sm"
          onClick={() => setShowAdd(true)}
          disabled={!persistEnabled}
          className="whitespace-nowrap"
        >
          <span className="material-symbols-outlined text-[14px]">add</span>
          {t('settings.evolution.cloud.add')}
        </Button>
      }
    >
      {targets.length === 0 ? (
        <div className="text-xs text-[var(--color-text-tertiary)] mb-3">
          {t('settings.evolution.cloud.empty')}
        </div>
      ) : (
        <div className="flex flex-col gap-2 mb-4">
          {targets.map((target) => (
            <div
              key={target.id}
              className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-2"
            >
              <div className="flex-1 min-w-0">
                <div className="text-xs font-semibold text-[var(--color-text-primary)] truncate">
                  {target.name}
                  <span className="ml-2 text-[10px] text-[var(--color-text-tertiary)] font-normal">
                    {target.kind}
                  </span>
                </div>
                <div className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                  {target.endpoint || '(no endpoint)'} · last pushed {formatTimestamp(target.lastPushedAt)}
                </div>
              </div>
              <div className="flex items-center gap-1">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    setPushPickerForTarget(target)
                    setPushExportId(exports[0]?.id ?? '')
                  }}
                  disabled={!persistEnabled || exports.length === 0}
                >
                  {t('settings.evolution.cloud.push')}
                </Button>
                <Button variant="ghost" size="sm" onClick={() => setEditing(target)}>
                  edit
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-[var(--color-error)]"
                  onClick={() => setPendingDelete(target)}
                >
                  delete
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)] mb-2">
        {t('settings.evolution.cloud.history')}
      </div>
      {pushHistory.length === 0 ? (
        <div className="text-xs text-[var(--color-text-tertiary)]">—</div>
      ) : (
        <div className="flex flex-col gap-1">
          {pushHistory.slice(0, 8).map((r) => (
            <div
              key={r.id}
              className="flex items-center justify-between gap-3 text-xs rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-1.5"
            >
              <div className="flex-1 min-w-0">
                <div className="font-mono text-[var(--color-text-primary)] truncate">
                  {targetMap.get(r.targetId)?.name ?? r.targetId} · {r.status}
                  {r.latencyMs !== null && ` · ${r.latencyMs}ms`}
                </div>
                <div className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                  {formatTimestamp(r.ts)} · {r.exportId}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {(showAdd || editing) && (
        <CloudTargetEditor
          initial={editing}
          onClose={() => {
            setShowAdd(false)
            setEditing(null)
          }}
          onSubmit={async (payload) => {
            await upsert(payload)
            setShowAdd(false)
            setEditing(null)
          }}
        />
      )}

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={async () => {
          if (pendingDelete) {
            await remove(pendingDelete.id)
            setPendingDelete(null)
          }
        }}
        title="Delete target"
        body={pendingDelete ? `Delete cloud target "${pendingDelete.name}"?` : ''}
        confirmLabel="Delete"
        cancelLabel="Cancel"
        confirmVariant="danger"
      />

      <Modal
        open={pushPickerForTarget !== null}
        onClose={() => setPushPickerForTarget(null)}
        title={t('settings.evolution.cloud.push')}
        width={380}
        footer={
          <>
            <Button variant="secondary" onClick={() => setPushPickerForTarget(null)}>
              Cancel
            </Button>
            <Button
              onClick={async () => {
                if (pushPickerForTarget && pushExportId) {
                  await push(pushPickerForTarget.id, pushExportId)
                  setPushPickerForTarget(null)
                }
              }}
              disabled={!pushExportId}
            >
              {t('settings.evolution.cloud.push')}
            </Button>
          </>
        }
      >
        <div className="text-xs text-[var(--color-text-secondary)] mb-2">Select an export to push.</div>
        <select
          value={pushExportId}
          onChange={(e) => setPushExportId(e.target.value)}
          className="w-full text-xs px-3 py-2 rounded-md bg-[var(--color-surface-container)] border border-[var(--color-border)] text-[var(--color-text-primary)]"
        >
          {exports.map((e) => (
            <option key={e.id} value={e.id}>
              {e.format} · {formatNumber(e.sampleCount)} samples · {formatTimestamp(e.createdAt)}
            </option>
          ))}
        </select>
      </Modal>
    </SectionShell>
  )
}

function CloudTargetEditor({
  initial,
  onClose,
  onSubmit,
}: {
  initial: CloudTarget | null
  onClose: () => void
  onSubmit: (payload: Partial<CloudTarget> & {
    name: string
    kind: CloudTarget['kind']
    endpoint: string
    enabled: boolean
  }) => Promise<void>
}) {
  const [name, setName] = useState(initial?.name ?? '')
  const [kind, setKind] = useState<CloudTarget['kind']>(initial?.kind ?? 'webhook')
  const [endpoint, setEndpoint] = useState(initial?.endpoint ?? '')
  const [secretRef, setSecretRef] = useState(initial?.secretRef ?? '')
  const [enabled, setEnabled] = useState(initial?.enabled ?? true)
  const [autoPush, setAutoPush] = useState(initial?.autoPush ?? false)
  const [submitting, setSubmitting] = useState(false)

  const handleSubmit = async () => {
    setSubmitting(true)
    try {
      await onSubmit({
        id: initial?.id,
        name,
        kind,
        endpoint,
        secretRef: secretRef || null,
        enabled,
        autoPush,
      })
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open={true}
      onClose={onClose}
      title={initial ? 'Edit cloud target' : 'Add cloud target'}
      width={520}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => void handleSubmit()} disabled={!name || submitting} loading={submitting}>
            Save
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} />
        <div>
          <label className="text-xs font-medium text-[var(--color-text-primary)] mb-1 block">Kind</label>
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as CloudTarget['kind'])}
            className="w-full text-xs px-3 py-2 rounded-md bg-[var(--color-surface-container)] border border-[var(--color-border)] text-[var(--color-text-primary)]"
          >
            <option value="openai_files">OpenAI Files</option>
            <option value="huggingface_dataset">Hugging Face Dataset</option>
            <option value="rl_dataset_server">RL Dataset Server</option>
            <option value="tinker">Tinker</option>
            <option value="fireworks">Fireworks</option>
            <option value="webhook">Custom Webhook</option>
          </select>
        </div>
        <Input label="Endpoint" value={endpoint} onChange={(e) => setEndpoint(e.target.value)} />
        <Input
          label="Secret ref (env var name)"
          value={secretRef}
          onChange={(e) => setSecretRef(e.target.value)}
          placeholder="e.g. OPENAI_API_KEY"
        />
        <ToggleRow label="Enabled" value={enabled} onChange={setEnabled} />
        <ToggleRow label="Auto push" value={autoPush} onChange={setAutoPush} />
      </div>
    </Modal>
  )
}

function SliderRow({
  label,
  value,
  min,
  max,
  step,
  hint,
  format,
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  hint?: string
  format?: (v: number) => string
  onChange: (next: number) => void
}) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center justify-between text-xs">
        <span className="text-[var(--color-text-primary)]">{label}</span>
        <span className="text-[var(--color-text-tertiary)] tabular-nums">
          {format ? format(value) : value}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full accent-[var(--color-brand)]"
      />
      {hint && <span className="text-[10px] text-[var(--color-text-tertiary)]">{hint}</span>}
    </div>
  )
}

function SegmentedRow<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string
  value: T
  options: Array<{ value: T; label: string }>
  onChange: (next: T) => void
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs text-[var(--color-text-primary)]">{label}</span>
      <div className="inline-flex rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] p-0.5 self-start">
        {options.map((opt) => (
          <button
            key={opt.value}
            onClick={() => onChange(opt.value)}
            className={`text-xs px-3 py-1 rounded transition-all ${
              value === opt.value
                ? 'bg-[var(--color-brand)] text-white shadow-sm'
                : 'text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]'
            }`}
          >
            {opt.label}
          </button>
        ))}
      </div>
    </div>
  )
}

function RecyclingCard({
  config,
  items,
  total,
  overview,
  persistEnabled,
}: {
  config: ExperienceRecyclingConfig | null
  items: RecycledExperienceItem[]
  total: number
  overview: EvolutionOverview | null
  persistEnabled: boolean
}) {
  const t = useTranslation()
  const updateRecyclingConfig = useEvolutionStore((s) => s.updateRecyclingConfig)
  const purgeRecycling = useEvolutionStore((s) => s.purgeRecycling)
  const setPersistence = useEvolutionStore((s) => s.setPersistence)
  const [busy, setBusy] = useState(false)
  const enabled = config?.enabled ?? false
  const needsPersist = enabled && !persistEnabled
  const recycling = overview?.recycling
  const recent24h = recycling?.recent24hHarvested ?? 0
  const totalHarvested = Math.max(recycling?.totalHarvested ?? 0, total)

  if (!config) {
    return (
      <SectionShell
        title={t('settings.evolution.recycling.title')}
        description={t('settings.evolution.recycling.description')}
      >
        <div className="text-xs text-[var(--color-text-tertiary)]">…</div>
      </SectionShell>
    )
  }

  const outcomeLabel = (outcome: RecycledExperienceItem['outcome']): string => {
    if (outcome === 'success') return t('settings.evolution.recycling.outcome.success')
    if (outcome === 'failure') return t('settings.evolution.recycling.outcome.failure')
    return t('settings.evolution.recycling.outcome.neutral')
  }

  const handlePurge = async () => {
    setBusy(true)
    try {
      await purgeRecycling()
    } finally {
      setBusy(false)
    }
  }

  return (
    <SectionShell
      title={t('settings.evolution.recycling.title')}
      description={t('settings.evolution.recycling.description')}
      rightAction={
        <button
          onClick={() => void updateRecyclingConfig({ enabled: !enabled })}
          className={`whitespace-nowrap text-xs font-semibold px-3 py-1.5 rounded-lg border transition-all ${
            enabled
              ? 'border-[var(--color-brand)] bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
              : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
          }`}
        >
          {enabled
            ? t('settings.evolution.persistence.toggleOn')
            : t('settings.evolution.persistence.toggleOff')}
        </button>
      }
    >
      <div className="flex flex-col gap-4">
        {needsPersist && (
          <div className="text-xs text-[var(--color-warning)] px-3 py-2 rounded-md bg-[var(--color-warning)]/15 border border-[var(--color-warning)]/30 flex items-start gap-2">
            <div className="flex-1">
              <div className="font-semibold mb-0.5">
                {t('settings.evolution.recycling.persistRequired.title')}
              </div>
              <div>{t('settings.evolution.recycling.persistRequired.body')}</div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void setPersistence(true)}
            >
              {t('settings.evolution.recycling.persistRequired.enable')}
            </Button>
          </div>
        )}
        <div className="flex flex-wrap gap-2">
          <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]">
            {t('settings.evolution.recycling.runtime.todayHarvested', {
              count: String(recent24h),
            })}
          </span>
          <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]">
            {t('settings.evolution.recycling.runtime.totalHarvested', {
              count: String(totalHarvested),
            })}
          </span>
          {recycling?.lastHarvestAt && (
            <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 rounded bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]">
              {t('settings.evolution.recycling.runtime.lastHarvestAt', {
                ts: formatRelativeTime(recycling.lastHarvestAt),
              })}
            </span>
          )}
        </div>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <SliderRow
            label={t('settings.evolution.recycling.sampleRate')}
            value={config.sampleRate}
            min={0}
            max={1}
            step={0.05}
            hint={t('settings.evolution.recycling.sampleRateHelp')}
            format={(v) => `${Math.round(v * 100)}%`}
            onChange={(v) => void updateRecyclingConfig({ sampleRate: v })}
          />
          <SliderRow
            label={t('settings.evolution.recycling.minReward')}
            value={config.minReward}
            min={-1}
            max={1}
            step={0.05}
            hint={t('settings.evolution.recycling.minRewardHelp')}
            format={(v) => v.toFixed(2)}
            onChange={(v) => void updateRecyclingConfig({ minReward: v })}
          />
          <NumberRow
            label={t('settings.evolution.recycling.maxRetained')}
            value={config.maxRetained}
            min={50}
            max={10000}
            step={50}
            onChange={(v) => void updateRecyclingConfig({ maxRetained: v })}
          />
          <NumberRow
            label={t('settings.evolution.recycling.maxReplayInPrompt')}
            value={config.maxReplayInPrompt}
            min={0}
            max={16}
            onChange={(v) => void updateRecyclingConfig({ maxReplayInPrompt: v })}
          />
          <NumberRow
            label={t('settings.evolution.recycling.replayTokenBudget')}
            value={config.replayTokenBudget}
            min={128}
            max={8000}
            step={64}
            onChange={(v) => void updateRecyclingConfig({ replayTokenBudget: v })}
          />
        </div>
        <div className="flex flex-col gap-2">
          <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
            {t('settings.evolution.recycling.privacy')}
          </div>
          <ToggleRow
            label={t('settings.evolution.recycling.redactPaths')}
            value={config.redactWorkspacePaths}
            onChange={(v) => void updateRecyclingConfig({ redactWorkspacePaths: v })}
          />
          <ToggleRow
            label={t('settings.evolution.recycling.redactSecrets')}
            value={config.redactSecrets}
            onChange={(v) => void updateRecyclingConfig({ redactSecrets: v })}
          />
          <ToggleRow
            label={t('settings.evolution.recycling.redactUserText')}
            value={config.redactUserText}
            onChange={(v) => void updateRecyclingConfig({ redactUserText: v })}
          />
          <ToggleRow
            label={t('settings.evolution.recycling.includeSuccesses')}
            value={config.includeSuccesses}
            onChange={(v) => void updateRecyclingConfig({ includeSuccesses: v })}
          />
          <ToggleRow
            label={t('settings.evolution.recycling.includeFailures')}
            value={config.includeFailures}
            onChange={(v) => void updateRecyclingConfig({ includeFailures: v })}
          />
        </div>
        <div className="flex flex-col gap-2">
          <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
            {t('settings.evolution.recycling.weights')}
          </div>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <SliderRow
              label={t('settings.evolution.recycling.weightQuality')}
              value={config.weightQuality}
              min={0}
              max={1}
              step={0.05}
              format={(v) => v.toFixed(2)}
              onChange={(v) => void updateRecyclingConfig({ weightQuality: v })}
            />
            <SliderRow
              label={t('settings.evolution.recycling.weightRecency')}
              value={config.weightRecency}
              min={0}
              max={1}
              step={0.05}
              format={(v) => v.toFixed(2)}
              onChange={(v) => void updateRecyclingConfig({ weightRecency: v })}
            />
            <SliderRow
              label={t('settings.evolution.recycling.weightDiversity')}
              value={config.weightDiversity}
              min={0}
              max={1}
              step={0.05}
              format={(v) => v.toFixed(2)}
              onChange={(v) => void updateRecyclingConfig({ weightDiversity: v })}
            />
          </div>
        </div>
        <div className="flex items-center justify-between gap-2 border-t border-[var(--color-border)] pt-3">
          <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
            {t('settings.evolution.recycling.recent')} ·{' '}
            {t('settings.evolution.recycling.total', { count: String(total) })}
          </div>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void handlePurge()}
            disabled={busy || total === 0}
            loading={busy}
            className="text-[var(--color-error)]"
          >
            {t('settings.evolution.recycling.purge')}
          </Button>
        </div>
        {items.length === 0 ? (
          <div className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.evolution.recycling.empty')}
          </div>
        ) : (
          <div className="flex flex-col gap-1">
            {items.slice(0, 8).map((item) => (
              <div
                key={item.id}
                className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-2"
              >
                <div className="flex items-center justify-between gap-2 mb-1">
                  <div className="text-xs font-semibold text-[var(--color-text-primary)] truncate flex-1 min-w-0">
                    {item.headline}
                  </div>
                  <span
                    className={`text-[10px] px-1.5 py-0.5 rounded font-medium uppercase tracking-wider ${
                      item.outcome === 'success'
                        ? 'bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
                        : item.outcome === 'failure'
                          ? 'bg-[var(--color-error)]/15 text-[var(--color-error)]'
                          : 'bg-[var(--color-surface-hover)] text-[var(--color-text-tertiary)]'
                    }`}
                  >
                    {outcomeLabel(item.outcome)}
                  </span>
                </div>
                <div className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                  {item.codingMode ?? 'global'} · reward {item.reward.toFixed(2)} · hits {item.hits} ·{' '}
                  {formatTimestamp(item.createdAt)}
                </div>
                {item.tags.length > 0 && (
                  <div className="text-[10px] text-[var(--color-text-tertiary)] mt-0.5 truncate">
                    {item.tags.join(' · ')}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </SectionShell>
  )
}

function ReflectionCard({
  config,
  runs,
  summary,
  availableModels,
  availableProviders,
  providersConfigured,
  persistEnabled,
}: {
  config: SelfReflectionConfig | null
  runs: ReflectionRunItem[]
  summary: ReflectionSummary | null
  availableModels: AvailableModelEntry[]
  availableProviders: AvailableProviderEntry[]
  providersConfigured: number
  persistEnabled: boolean
}) {
  const t = useTranslation()
  const updateReflectionConfig = useEvolutionStore((s) => s.updateReflectionConfig)
  const triggerReflection = useEvolutionStore((s) => s.triggerReflection)
  const setPersistence = useEvolutionStore((s) => s.setPersistence)
  const reflectionStoreError = useEvolutionStore((s) => s.reflectionStoreError)
  const [busy, setBusy] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)

  if (!config) {
    return (
      <SectionShell
        title={t('settings.evolution.reflection.title')}
        description={t('settings.evolution.reflection.description')}
      >
        <div className="text-xs text-[var(--color-text-tertiary)]">…</div>
      </SectionShell>
    )
  }

  const enabled = config.enabled
  const writebackSet = new Set<ReflectionWritebackTargetId>(config.writebackTargets)
  const needsPersist = enabled && !persistEnabled
  const hasModels = availableModels.length > 0
  const runDisabled =
    busy || !enabled || !persistEnabled || !hasModels || reflectionStoreError !== null

  const mapRunErrorKey = (code: string): TranslationKey | null => {
    const sanitized = code.trim()
    if (!sanitized) return null
    const key = `settings.evolution.reflection.runError.${sanitized}` as TranslationKey
    return key
  }
  const mapSkipReasonKey = (code: string): TranslationKey | null => {
    const sanitized = code.trim()
    if (!sanitized) return null
    const key = `settings.evolution.reflection.skipReason.${sanitized}` as TranslationKey
    return key
  }
  const translateOrFallback = (key: TranslationKey, fallback: string): string => {
    const translated = t(key)
    return translated === key ? fallback : translated
  }

  const toggleTarget = (target: ReflectionWritebackTargetId, on: boolean) => {
    const next = new Set(writebackSet)
    if (on) {
      next.add(target)
    } else {
      next.delete(target)
    }
    const nextArr = Array.from(next)
    void updateReflectionConfig({
      writebackTargets: nextArr.length > 0 ? nextArr : (['lessons'] as ReflectionWritebackTargetId[]),
    })
  }

  const handleRun = async () => {
    setBusy(true)
    setFeedback(null)
    try {
      const runId = await triggerReflection(null)
      if (runId) {
        setFeedback(t('settings.evolution.reflection.runQueued', { runId }))
      } else {
        setFeedback(t('settings.evolution.reflection.runFailed', { error: '' }))
      }
    } catch (err) {
      const raw = err instanceof Error ? err.message : 'unknown'
      const parts = raw.split(':')
      const code = (parts[0] ?? raw).trim()
      const detail = parts.slice(1).join(':').trim()
      const key = mapRunErrorKey(code)
      let resolved = raw
      if (key) {
        const translated = t(key)
        if (translated !== key) {
          resolved = detail ? `${translated} (${detail})` : translated
        }
      }
      setFeedback(
        t('settings.evolution.reflection.runFailed', {
          error: resolved,
        }),
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <SectionShell
      title={t('settings.evolution.reflection.title')}
      description={t('settings.evolution.reflection.description')}
      rightAction={
        <button
          onClick={() => void updateReflectionConfig({ enabled: !enabled })}
          className={`whitespace-nowrap text-xs font-semibold px-3 py-1.5 rounded-lg border transition-all ${
            enabled
              ? 'border-[var(--color-brand)] bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
              : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
          }`}
        >
          {enabled
            ? t('settings.evolution.persistence.toggleOn')
            : t('settings.evolution.persistence.toggleOff')}
        </button>
      }
    >
      <div className="flex flex-col gap-4">
        {needsPersist && (
          <div className="text-xs text-[var(--color-warning)] px-3 py-2 rounded-md bg-[var(--color-warning)]/15 border border-[var(--color-warning)]/30 flex items-start gap-2">
            <div className="flex-1">
              <div className="font-semibold mb-0.5">
                {t('settings.evolution.reflection.persistRequired.title')}
              </div>
              <div>{t('settings.evolution.reflection.persistRequired.body')}</div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void setPersistence(true)}
            >
              {t('settings.evolution.reflection.persistRequired.enable')}
            </Button>
          </div>
        )}
        {summary && (
          <div className="grid grid-cols-4 gap-3">
            <Stat
              label={t('settings.evolution.reflection.summary.totalRuns')}
              value={formatNumber(summary.totalRuns)}
            />
            <Stat
              label={t('settings.evolution.reflection.summary.completed')}
              value={formatNumber(summary.completedRuns)}
            />
            <Stat
              label={t('settings.evolution.reflection.summary.failed')}
              value={formatNumber(summary.failedRuns)}
            />
            <Stat
              label={t('settings.evolution.reflection.summary.lessons')}
              value={formatNumber(summary.totalLessonsProduced)}
            />
          </div>
        )}
        {summary && (
          <div className="flex flex-wrap gap-3 text-[11px] text-[var(--color-text-secondary)]">
            <span>
              <span className="text-[var(--color-text-tertiary)]">
                {t('settings.evolution.reflection.summary.lastRunAt')}:
              </span>{' '}
              {summary.lastRunAt ? formatRelativeTime(summary.lastRunAt) : '—'}
            </span>
            <span>
              <span className="text-[var(--color-text-tertiary)]">
                {t('settings.evolution.reflection.summary.lastStatus')}:
              </span>{' '}
              {summary.lastStatus
                ? translateOrFallback(
                    `settings.evolution.reflection.run.status.${summary.lastStatus}` as TranslationKey,
                    summary.lastStatus,
                  )
                : '—'}
            </span>
            <span>
              <span className="text-[var(--color-text-tertiary)]">
                {t('settings.evolution.reflection.summary.avgLessonsPerRun')}:
              </span>{' '}
              {summary.avgLessonsPerRun !== null && summary.avgLessonsPerRun !== undefined
                ? summary.avgLessonsPerRun.toFixed(2)
                : '—'}
            </span>
          </div>
        )}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <SegmentedRow<ReflectionTriggerModeId>
            label={t('settings.evolution.reflection.triggerMode')}
            value={config.triggerMode}
            options={[
              { value: 'manual', label: t('settings.evolution.reflection.triggerMode.manual') },
              { value: 'auto', label: t('settings.evolution.reflection.triggerMode.auto') },
              { value: 'scheduled', label: t('settings.evolution.reflection.triggerMode.scheduled') },
            ]}
            onChange={(v) => void updateReflectionConfig({ triggerMode: v })}
          />
          <SegmentedRow<ReflectionDepthId>
            label={t('settings.evolution.reflection.depth')}
            value={config.depth}
            options={[
              { value: 'quick', label: t('settings.evolution.reflection.depth.quick') },
              { value: 'deep', label: t('settings.evolution.reflection.depth.deep') },
            ]}
            onChange={(v) => void updateReflectionConfig({ depth: v })}
          />
        </div>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <ProviderPicker
            label={t('settings.evolution.reflection.provider')}
            value={config.reflectionProvider}
            providers={availableProviders}
            onChange={(next) => {
              void updateReflectionConfig({ reflectionProvider: next }).catch(
                (error: unknown) => {
                  console.warn('[evolution] reflection provider update failed', error)
                },
              )
            }}
          />
          <ModelPicker
            label={t('settings.evolution.reflection.model')}
            value={config.reflectionModel}
            models={availableModels}
            providersConfigured={providersConfigured}
            helperText={t('settings.evolution.reflection.modelHelp')}
            onChange={(next) => {
              void updateReflectionConfig({ reflectionModel: next }).catch((error: unknown) => {
                console.warn('[evolution] reflection model update failed', error)
              })
            }}
          />
        </div>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <NumberRow
            label={t('settings.evolution.reflection.scheduleIntervalMinutes')}
            value={config.scheduleIntervalMinutes}
            min={5}
            max={1440}
            step={5}
            onChange={(v) => void updateReflectionConfig({ scheduleIntervalMinutes: v })}
          />
          <NumberRow
            label={t('settings.evolution.reflection.minTurnsForAuto')}
            value={config.minTurnsForAuto}
            min={1}
            max={64}
            onChange={(v) => void updateReflectionConfig({ minTurnsForAuto: v })}
          />
          <NumberRow
            label={t('settings.evolution.reflection.failureThreshold')}
            value={config.failureThreshold}
            min={1}
            max={32}
            onChange={(v) => void updateReflectionConfig({ failureThreshold: v })}
          />
          <NumberRow
            label={t('settings.evolution.reflection.lookbackTurns')}
            value={config.lookbackTurns}
            min={1}
            max={64}
            onChange={(v) => void updateReflectionConfig({ lookbackTurns: v })}
          />
          <NumberRow
            label={t('settings.evolution.reflection.maxLessonsPerRun')}
            value={config.maxLessonsPerRun}
            min={1}
            max={16}
            onChange={(v) => void updateReflectionConfig({ maxLessonsPerRun: v })}
          />
          <NumberRow
            label={t('settings.evolution.reflection.maxTotalLessons')}
            value={config.maxTotalLessons}
            min={50}
            max={5000}
            step={50}
            onChange={(v) => void updateReflectionConfig({ maxTotalLessons: v })}
          />
        </div>
        <ToggleRow
          label={t('settings.evolution.reflection.includeUserThumbsDown')}
          value={config.includeUserThumbsDown}
          onChange={(v) => void updateReflectionConfig({ includeUserThumbsDown: v })}
        />
        <div className="flex flex-col gap-2">
          <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
            {t('settings.evolution.reflection.writeback')}
          </div>
          <div className="flex flex-wrap gap-2">
            {(['lessons', 'skills', 'rules', 'memory'] as ReflectionWritebackTargetId[]).map((target) => {
              const active = writebackSet.has(target)
              return (
                <button
                  key={target}
                  onClick={() => toggleTarget(target, !active)}
                  className={`text-xs px-3 py-1.5 rounded-md border transition-all ${
                    active
                      ? 'border-[var(--color-brand)] bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
                      : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                  }`}
                >
                  {t(`settings.evolution.reflection.writeback.${target}`)}
                </button>
              )
            })}
          </div>
        </div>
        <div className="flex items-center justify-between gap-2 border-t border-[var(--color-border)] pt-3">
          <span className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
            {t('settings.evolution.reflection.recent')}
          </span>
          <div className="flex flex-col items-end gap-1">
            <Button
              onClick={() => void handleRun()}
              disabled={runDisabled}
              loading={busy}
              title={
                !hasModels
                  ? t('settings.evolution.modelPicker.addProviderFirst')
                  : undefined
              }
            >
              {t('settings.evolution.reflection.runNow')}
            </Button>
            {!hasModels && (
              <a
                href="#/settings/providers"
                className="text-[10px] text-[var(--color-text-tertiary)] underline"
              >
                {t('settings.evolution.modelEmpty.gotoProviders')}
              </a>
            )}
          </div>
        </div>
        {reflectionStoreError && (
          <div className="text-xs text-[var(--color-error)] px-3 py-2 rounded-md bg-[var(--color-error)]/10 border border-[var(--color-error)]/30">
            {reflectionStoreError}
          </div>
        )}
        {feedback && (
          <div className="text-xs text-[var(--color-text-secondary)] bg-[var(--color-surface-container)] rounded-md px-2 py-1">
            {feedback}
          </div>
        )}
        {runs.length === 0 ? (
          <div className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.evolution.reflection.empty')}
          </div>
        ) : (
          <div className="flex flex-col gap-1">
            {runs.slice(0, 8).map((run) => {
              const statusKey = `settings.evolution.reflection.run.status.${run.status}` as TranslationKey
              const skipLabel = run.error
                ? (() => {
                    const code = run.error?.split(';')[0]?.trim() ?? ''
                    const key = mapSkipReasonKey(code)
                    if (key) {
                      const translated = t(key)
                      if (translated !== key) return translated
                    }
                    const runErr = mapRunErrorKey(code)
                    if (runErr) {
                      const translated = t(runErr)
                      if (translated !== runErr) return translated
                    }
                    return run.error
                  })()
                : null
              return (
                <div
                  key={run.id}
                  className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-2"
                >
                  <div className="flex items-center justify-between gap-2 mb-0.5">
                    <div className="text-xs font-mono text-[var(--color-text-primary)] truncate flex-1 min-w-0">
                      {run.id}
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      {run.lessonsProduced > 0 && (
                        <span className="text-[10px] text-[var(--color-brand)] font-medium">
                          +{run.lessonsProduced} lessons
                        </span>
                      )}
                      <span
                        className={`text-[10px] px-1.5 py-0.5 rounded font-medium uppercase tracking-wider ${statusBadgeClass(
                          run.status,
                        )}`}
                      >
                        {translateOrFallback(statusKey, run.status)}
                      </span>
                    </div>
                  </div>
                  <div className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                    {run.trigger} · {run.depth} · turns {run.turnsAnalyzed} · {formatRelativeTime(run.startedAt)}
                  </div>
                  {run.summary && (
                    <div className="text-[11px] text-[var(--color-text-secondary)] mt-1 line-clamp-2">
                      {run.summary}
                    </div>
                  )}
                  {skipLabel && (
                    <div className="text-[11px] text-[var(--color-error)] mt-1 line-clamp-2">
                      {skipLabel}
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </SectionShell>
  )
}
