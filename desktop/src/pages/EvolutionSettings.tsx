import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from '../i18n'
import { Button } from '../components/shared/Button'
import { Modal } from '../components/shared/Modal'
import { Input } from '../components/shared/Input'
import { ConfirmDialog } from '../components/shared/ConfirmDialog'
import { useEvolutionStore } from '../stores/evolutionStore'
import type {
  CloudTarget,
  EvolutionExportFormatId,
  PurgeScopeId,
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
  const fetchAll = useEvolutionStore((s) => s.fetchAll)

  useEffect(() => {
    void fetchAll()
  }, [fetchAll])

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
      <MaintenanceCard persistEnabled={config?.persistTrainingData ?? false} />
      <EngineConfigCard config={config} />
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
  overview: ReturnType<typeof useEvolutionStore.getState>['overview']
}) {
  const t = useTranslation()
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
}: {
  config: ReturnType<typeof useEvolutionStore.getState>['config']
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
        <Input
          label={t('settings.evolution.config.judgeModel')}
          value={config.judgeModel ?? ''}
          onChange={(e) => void updateConfig({ judgeModel: e.target.value || null })}
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
