// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import {
  useComputerAnalysisStore,
  type AutomationPlan,
  type SkillPlan,
} from '../../stores/computerAnalysisStore'

async function pickExportDir(): Promise<string | null> {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ directory: true, multiple: false })
    return typeof selected === 'string' ? selected : null
  } catch {
    return null
  }
}

export function BuilderView({ onClose }: { onClose: () => void }) {
  const t = useTranslation()
  const targets = useComputerAnalysisStore((s) => s.targets)
  const loadTargets = useComputerAnalysisStore((s) => s.loadTargets)
  const buildKind = useComputerAnalysisStore((s) => s.buildKind)
  const buildArchitecture = useComputerAnalysisStore((s) => s.buildArchitecture)
  const setBuildTarget = useComputerAnalysisStore((s) => s.setBuildTarget)
  const buildPhase = useComputerAnalysisStore((s) => s.buildPhase)
  const buildMessage = useComputerAnalysisStore((s) => s.buildMessage)
  const buildError = useComputerAnalysisStore((s) => s.buildError)
  const skillPlan = useComputerAnalysisStore((s) => s.skillPlan)
  const automationPlan = useComputerAnalysisStore((s) => s.automationPlan)
  const builtPath = useComputerAnalysisStore((s) => s.builtPath)
  const builtPlacement = useComputerAnalysisStore((s) => s.builtPlacement)
  const propose = useComputerAnalysisStore((s) => s.propose)
  const refine = useComputerAnalysisStore((s) => s.refine)
  const updateSkillPlan = useComputerAnalysisStore((s) => s.updateSkillPlan)
  const updateAutomationPlan = useComputerAnalysisStore((s) => s.updateAutomationPlan)
  const create = useComputerAnalysisStore((s) => s.create)
  const resetBuild = useComputerAnalysisStore((s) => s.resetBuild)

  const [refineText, setRefineText] = useState('')

  useEffect(() => {
    void loadTargets()
  }, [loadTargets])

  const currentTarget = targets.find(
    (t) => t.kind === buildKind && t.architecture === buildArchitecture,
  )
  const placements = currentTarget?.placements ?? ['install']

  const handleClose = () => {
    resetBuild()
    onClose()
  }

  return (
    <div className="absolute inset-0 z-50 flex justify-center bg-black/50 p-6" onClick={handleClose}>
      <div
        className="flex h-full w-[min(680px,96vw)] flex-col overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
          <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">build</span>
          <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
            {t('computerUse.builder.title')}
          </div>
          <button
            type="button"
            onClick={handleClose}
            className="ml-auto inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
          >
            <span className="material-symbols-outlined text-[16px]">close</span>
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          <div className="mb-3">
            <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
              {t('computerUse.builder.pickTarget')}
            </div>
            <div className="flex flex-wrap gap-2">
              {targets.map((target) => {
                const active =
                  target.kind === buildKind && target.architecture === buildArchitecture
                return (
                  <button
                    key={`${target.architecture}-${target.kind}`}
                    type="button"
                    onClick={() =>
                      setBuildTarget(target.kind as 'skill' | 'automation', target.architecture)
                    }
                    className={`rounded-lg border px-3 py-2 text-left text-[11px] transition-colors ${
                      active
                        ? 'border-[var(--color-brand)] bg-[var(--color-primary-fixed)] text-[var(--color-text-primary)]'
                        : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-black/[0.04] dark:hover:bg-white/[0.06]'
                    }`}
                  >
                    <div className="font-semibold text-[var(--color-text-primary)]">
                      {target.label}
                    </div>
                  </button>
                )
              })}
            </div>
          </div>

          {buildError && (
            <div className="mb-3 rounded-lg border border-[var(--color-error)]/30 bg-[var(--color-error)]/10 px-3 py-2 text-[12px] text-[var(--color-error)]">
              {buildError}
            </div>
          )}

          {(buildPhase === 'idle' || buildPhase === 'error') && (
            <button
              type="button"
              onClick={propose}
              className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-4 py-2 text-[12px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90"
            >
              <span className="material-symbols-outlined text-[16px]">auto_awesome</span>
              {t('computerUse.builder.propose')}
            </button>
          )}

          {(buildPhase === 'planning' || buildPhase === 'creating') && (
            <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-3">
              <span className="material-symbols-outlined animate-spin text-[16px] text-[var(--color-brand)]">
                progress_activity
              </span>
              <span className="text-[12px] text-[var(--color-text-secondary)]">
                {buildMessage ||
                  (buildPhase === 'creating'
                    ? t('computerUse.builder.drafting')
                    : t('computerUse.builder.planning'))}
              </span>
            </div>
          )}

          {buildPhase === 'plan' && skillPlan && buildKind === 'skill' && (
            <SkillPlanEditor
              plan={skillPlan}
              onChange={updateSkillPlan}
              placements={placements}
              refineText={refineText}
              onRefineText={setRefineText}
              onRefine={() => {
                if (refineText.trim()) {
                  refine(refineText.trim())
                  setRefineText('')
                }
              }}
              onCreate={async (placement) => {
                if (placement === 'export') {
                  const dir = await pickExportDir()
                  if (!dir) return
                  create('export', dir)
                } else {
                  create('install')
                }
              }}
            />
          )}

          {buildPhase === 'plan' && automationPlan && buildKind === 'automation' && (
            <AutomationPlanEditor
              plan={automationPlan}
              onChange={updateAutomationPlan}
              placements={placements}
              refineText={refineText}
              onRefineText={setRefineText}
              onRefine={() => {
                if (refineText.trim()) {
                  refine(refineText.trim())
                  setRefineText('')
                }
              }}
              onCreate={async (placement) => {
                if (placement === 'export') {
                  const dir = await pickExportDir()
                  if (!dir) return
                  create('export', dir)
                } else {
                  create('install')
                }
              }}
            />
          )}

          {buildPhase === 'done' && (
            <div className="flex flex-col items-center gap-3 py-8 text-center">
              <span className="material-symbols-outlined text-[32px] text-[var(--color-success)]">
                check_circle
              </span>
              <div className="text-[12px] text-[var(--color-text-primary)]">
                {buildDoneMessage(t, buildKind, builtPlacement, builtPath)}
              </div>
              <button
                type="button"
                onClick={resetBuild}
                className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                {t('computerUse.builder.buildAnother')}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

function buildDoneMessage(
  t: ReturnType<typeof useTranslation>,
  kind: 'skill' | 'automation',
  placement: string | null,
  path: string | null,
): string {
  if (kind === 'automation') {
    return placement === 'export'
      ? t('computerUse.builder.doneAutomationExport', { path: path ?? '' })
      : t('computerUse.builder.doneAutomationInstall')
  }
  return placement === 'export'
    ? t('computerUse.builder.doneSkillExport', { path: path ?? '' })
    : t('computerUse.builder.doneSkillInstall', { path: path ?? '' })
}

function ValuePills({
  values,
  onChange,
}: {
  values: { id: string; name: string; value: string }[]
  onChange: (values: { id: string; name: string; value: string }[]) => void
}) {
  const t = useTranslation()
  if (values.length === 0) return null
  return (
    <div className="mt-2">
      <div className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
        {t('computerUse.builder.values')}
      </div>
      <div className="flex flex-col gap-1.5">
        {values.map((value, idx) => (
          <div key={value.id} className="flex items-center gap-2">
            <span className="shrink-0 rounded bg-[var(--color-brand)]/12 px-1.5 py-0.5 font-mono text-[10px] text-[var(--color-brand)]">
              {`{{${value.id}}}`}
            </span>
            <span className="shrink-0 text-[10px] text-[var(--color-text-tertiary)]">
              {value.name || value.id}
            </span>
            <input
              value={value.value}
              onChange={(e) => {
                const next = [...values]
                next[idx] = { ...value, value: e.target.value }
                onChange(next)
              }}
              placeholder={t('computerUse.builder.valuePlaceholder')}
              className="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
            />
          </div>
        ))}
      </div>
    </div>
  )
}

function RefineAndCreate({
  placements,
  refineText,
  onRefineText,
  onRefine,
  onCreate,
  isAutomation,
}: {
  placements: string[]
  refineText: string
  onRefineText: (value: string) => void
  onRefine: () => void
  onCreate: (placement: string) => void
  isAutomation: boolean
}) {
  const t = useTranslation()
  return (
    <div className="mt-3 flex flex-col gap-2 border-t border-[var(--color-border)] pt-3">
      <textarea
        value={refineText}
        onChange={(e) => onRefineText(e.target.value)}
        placeholder={t('computerUse.builder.refinePlaceholder')}
        rows={2}
        className="w-full resize-none rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
      />
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={onRefine}
          disabled={!refineText.trim()}
          className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] disabled:opacity-50 dark:hover:bg-white/[0.08]"
        >
          <span className="material-symbols-outlined text-[14px]">tune</span>
          {t('computerUse.builder.refine')}
        </button>
        {placements.includes('install') && (
          <button
            type="button"
            onClick={() => onCreate('install')}
            className="ml-auto inline-flex items-center gap-1 rounded-md bg-[var(--color-brand)] px-3 py-1.5 text-[11px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90"
          >
            <span className="material-symbols-outlined text-[14px]">add</span>
            {isAutomation
              ? t('computerUse.builder.installAutomation')
              : t('computerUse.builder.install')}
          </button>
        )}
        {placements.includes('export') && (
          <button
            type="button"
            onClick={() => onCreate('export')}
            className={`inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08] ${
              placements.includes('install') ? '' : 'ml-auto'
            }`}
          >
            <span className="material-symbols-outlined text-[14px]">download</span>
            {isAutomation
              ? t('computerUse.builder.exportAutomation')
              : t('computerUse.builder.export')}
          </button>
        )}
      </div>
    </div>
  )
}

function SkillPlanEditor({
  plan,
  onChange,
  placements,
  refineText,
  onRefineText,
  onRefine,
  onCreate,
}: {
  plan: SkillPlan
  onChange: (plan: SkillPlan) => void
  placements: string[]
  refineText: string
  onRefineText: (value: string) => void
  onRefine: () => void
  onCreate: (placement: string) => void
}) {
  const t = useTranslation()
  return (
    <div className="flex flex-col gap-2">
      <input
        value={plan.title}
        onChange={(e) => onChange({ ...plan, title: e.target.value })}
        className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[13px] font-semibold text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
      />
      <textarea
        value={plan.description}
        onChange={(e) => onChange({ ...plan, description: e.target.value })}
        placeholder={t('computerUse.builder.description')}
        rows={2}
        className="resize-none rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[12px] text-[var(--color-text-secondary)] outline-none focus:border-[var(--color-brand)]"
      />
      <ValuePills values={plan.values} onChange={(values) => onChange({ ...plan, values })} />
      <div>
        <div className="mb-1 mt-2 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
          {t('computerUse.builder.steps')}
        </div>
        <ol className="flex flex-col gap-1.5">
          {plan.steps.map((step, idx) => (
            <li
              key={idx}
              className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] p-2"
            >
              <div className="flex items-center gap-1.5">
                <span
                  className={`rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase ${
                    step.kind === 'action'
                      ? 'bg-[var(--color-warning)]/15 text-[var(--color-warning)]'
                      : 'bg-[var(--color-brand)]/12 text-[var(--color-brand)]'
                  }`}
                >
                  {step.kind === 'action'
                    ? t('computerUse.builder.action')
                    : t('computerUse.builder.calculation')}
                </span>
                <input
                  value={step.title}
                  onChange={(e) => {
                    const steps = [...plan.steps]
                    steps[idx] = { ...step, title: e.target.value }
                    onChange({ ...plan, steps })
                  }}
                  className="min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1 py-0.5 text-[12px] font-medium text-[var(--color-text-primary)] outline-none hover:border-[var(--color-border)] focus:border-[var(--color-brand)]"
                />
                {step.tool && (
                  <span className="shrink-0 font-mono text-[9px] text-[var(--color-text-tertiary)]">
                    {step.tool}
                  </span>
                )}
              </div>
              <textarea
                value={step.text}
                onChange={(e) => {
                  const steps = [...plan.steps]
                  steps[idx] = { ...step, text: e.target.value }
                  onChange({ ...plan, steps })
                }}
                rows={2}
                className="mt-1 w-full resize-none rounded-md border border-transparent bg-transparent px-1 py-0.5 text-[11px] text-[var(--color-text-secondary)] outline-none hover:border-[var(--color-border)] focus:border-[var(--color-brand)]"
              />
            </li>
          ))}
        </ol>
      </div>
      <RefineAndCreate
        placements={placements}
        refineText={refineText}
        onRefineText={onRefineText}
        onRefine={onRefine}
        onCreate={onCreate}
        isAutomation={false}
      />
    </div>
  )
}

function AutomationPlanEditor({
  plan,
  onChange,
  placements,
  refineText,
  onRefineText,
  onRefine,
  onCreate,
}: {
  plan: AutomationPlan
  onChange: (plan: AutomationPlan) => void
  placements: string[]
  refineText: string
  onRefineText: (value: string) => void
  onRefine: () => void
  onCreate: (placement: string) => void
}) {
  const t = useTranslation()
  return (
    <div className="flex flex-col gap-2">
      <input
        value={plan.title}
        onChange={(e) => onChange({ ...plan, title: e.target.value })}
        className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[13px] font-semibold text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
      />
      <textarea
        value={plan.description}
        onChange={(e) => onChange({ ...plan, description: e.target.value })}
        placeholder={t('computerUse.builder.description')}
        rows={2}
        className="resize-none rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[12px] text-[var(--color-text-secondary)] outline-none focus:border-[var(--color-brand)]"
      />
      <ScheduleEditor plan={plan} onChange={onChange} />
      <ValuePills values={plan.values} onChange={(values) => onChange({ ...plan, values })} />
      <div>
        <div className="mb-1 mt-2 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
          {t('computerUse.builder.steps')}
        </div>
        <ol className="flex flex-col gap-1.5">
          {plan.steps.map((step, idx) => (
            <li
              key={idx}
              className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] p-2"
            >
              <input
                value={step.label}
                onChange={(e) => {
                  const steps = [...plan.steps]
                  steps[idx] = { ...step, label: e.target.value }
                  onChange({ ...plan, steps })
                }}
                className="w-full rounded-md border border-transparent bg-transparent px-1 py-0.5 text-[12px] font-medium text-[var(--color-text-primary)] outline-none hover:border-[var(--color-border)] focus:border-[var(--color-brand)]"
              />
              <textarea
                value={step.prompt}
                onChange={(e) => {
                  const steps = [...plan.steps]
                  steps[idx] = { ...step, prompt: e.target.value }
                  onChange({ ...plan, steps })
                }}
                rows={2}
                className="mt-1 w-full resize-none rounded-md border border-transparent bg-transparent px-1 py-0.5 text-[11px] text-[var(--color-text-secondary)] outline-none hover:border-[var(--color-border)] focus:border-[var(--color-brand)]"
              />
            </li>
          ))}
        </ol>
      </div>
      <RefineAndCreate
        placements={placements}
        refineText={refineText}
        onRefineText={onRefineText}
        onRefine={onRefine}
        onCreate={onCreate}
        isAutomation
      />
    </div>
  )
}

function ScheduleEditor({
  plan,
  onChange,
}: {
  plan: AutomationPlan
  onChange: (plan: AutomationPlan) => void
}) {
  const t = useTranslation()
  const schedule = plan.schedule as Record<string, unknown>
  const kind = (schedule.kind as string) || 'single'
  const time = (schedule.time as { hour: number; minute: number }) ||
    (schedule.anchor as { hour: number; minute: number }) || { hour: 9, minute: 0 }

  const setKind = (next: string) => {
    const base = { kind: next, naturalLanguage: '', days: [] as number[] }
    if (next === 'single') {
      onChange({ ...plan, schedule: { ...base, time } })
    } else if (next === 'interval') {
      onChange({ ...plan, schedule: { ...base, intervalMinutes: 60, anchor: time } })
    } else {
      onChange({ ...plan, schedule: { ...base, times: [time] } })
    }
  }

  const setTime = (hour: number, minute: number) => {
    const next = { ...schedule }
    if (kind === 'interval') {
      next.anchor = { hour, minute }
    } else if (kind === 'multi') {
      next.times = [{ hour, minute }]
    } else {
      next.time = { hour, minute }
    }
    onChange({ ...plan, schedule: next })
  }

  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] p-2.5">
      <div className="mb-2 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
        {t('computerUse.builder.schedule')}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <select
          value={kind}
          onChange={(e) => setKind(e.target.value)}
          className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none"
        >
          <option value="single">{t('computerUse.builder.scheduleSingle')}</option>
          <option value="interval">{t('computerUse.builder.scheduleInterval')}</option>
          <option value="multi">{t('computerUse.builder.scheduleMulti')}</option>
        </select>
        <label className="flex items-center gap-1 text-[11px] text-[var(--color-text-secondary)]">
          {t('computerUse.builder.time')}
          <input
            type="time"
            value={`${String(time.hour).padStart(2, '0')}:${String(time.minute).padStart(2, '0')}`}
            onChange={(e) => {
              const parts = e.target.value.split(':')
              const h = Number.parseInt(parts[0] ?? '', 10)
              const m = Number.parseInt(parts[1] ?? '', 10)
              if (Number.isFinite(h) && Number.isFinite(m)) setTime(h, m)
            }}
            className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none"
          />
        </label>
        {kind === 'interval' && (
          <label className="flex items-center gap-1 text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.builder.intervalMinutes')}
            <input
              type="number"
              min={1}
              max={1440}
              value={Number(schedule.intervalMinutes ?? 60)}
              onChange={(e) => {
                const value = Number.parseInt(e.target.value, 10)
                if (Number.isFinite(value)) {
                  onChange({ ...plan, schedule: { ...schedule, intervalMinutes: value } })
                }
              }}
              className="w-16 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none"
            />
          </label>
        )}
      </div>
    </div>
  )
}

export default BuilderView
