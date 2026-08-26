// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useEffect, type ReactNode, type SelectHTMLAttributes } from 'react'
import { useTaskStore } from '../../stores/taskStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useTabStore } from '../../stores/tabStore'
import { useAdapterStore } from '../../stores/adapterStore'
import { Modal } from '../shared/Modal'
import { Input } from '../shared/Input'
import { Button } from '../shared/Button'
import { SettingsSection } from '../settings/SettingsSection'
import { PromptEditor } from './PromptEditor'
import { DayOfWeekPicker } from './DayOfWeekPicker'
import { useTranslation } from '../../i18n'
import { describeCron, isValidCron, parseCron, type FrequencyKey } from '../../lib/cronDescribe'
import type { PermissionMode } from '../../types/settings'
import type { CodingModeId } from '../../types/codingMode'
import { DEFAULT_CODING_MODE } from '../../types/codingMode'
import type { CronTask, TaskPriority } from '../../types/task'

type Props = {
  open: boolean
  onClose: () => void
  editTask?: CronTask
}

type TriggerKind = 'schedule' | 'idle' | 'sessionEnd'

const MINUTE_INTERVALS = [5, 10, 15, 20, 30]
const HOUR_INTERVALS = [1, 2, 3, 4, 6, 8, 12]
const MINUTE_OFFSETS = [0, 15, 30, 45]

const SELECT_CLASS = 'w-full h-7 px-2.5 pr-8 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] appearance-none cursor-pointer'
const FIELD_CLASS = 'w-auto h-7 px-2.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]'

function buildCron(
  freq: FrequencyKey,
  time: string,
  opts: {
    minuteInterval: number
    hourInterval: number
    minuteOffset: number
    selectedDays: number[]
    monthDay: number
    customCron: string
  },
): string {
  const [hours, minutes] = time.split(':').map(Number)
  switch (freq) {
    case 'everyNMinutes':
      return `*/${opts.minuteInterval} * * * *`
    case 'everyNHours':
      return `${opts.minuteOffset} */${opts.hourInterval} * * *`
    case 'daily':
      return `${minutes} ${hours} * * *`
    case 'weekdays':
      return `${minutes} ${hours} * * 1-5`
    case 'specificDays':
      return `${minutes} ${hours} * * ${[...opts.selectedDays].sort((a, b) => a - b).join(',')}`
    case 'monthly':
      return `${minutes} ${hours} ${opts.monthDay} * *`
    case 'customCron':
      return opts.customCron.trim()
  }
}

function Field({
  label,
  hint,
  required,
  children,
}: {
  label?: string
  hint?: string
  required?: boolean
  children: ReactNode
}) {
  return (
    <div className="flex flex-col gap-1">
      {label && (
        <label className="text-xs font-medium text-[var(--color-text-primary)]">
          {label}
          {required && <span className="ml-0.5 text-[var(--color-error)]">*</span>}
        </label>
      )}
      {children}
      {hint && <span className="text-xs text-[var(--color-text-tertiary)]">{hint}</span>}
    </div>
  )
}

function NativeSelect({
  className = '',
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className={`relative ${className}`}>
      <select {...props} className={SELECT_CLASS}>
        {children}
      </select>
      <span className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]">
        expand_more
      </span>
    </div>
  )
}

export function NewTaskModal({ open, onClose, editTask }: Props) {
  const t = useTranslation()
  const createTask = useTaskStore((s) => s.createTask)
  const updateTask = useTaskStore((s) => s.updateTask)
  const sessions = useSessionStore((s) => s.sessions)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const activeSession = sessions.find((s) => s.id === activeTabId)
  const defaultWorkDir = activeSession?.workDir || ''
  const adapterConfig = useAdapterStore((s) => s.config)
  const fetchConfig = useAdapterStore((s) => s.fetchConfig)

  useEffect(() => {
    if (open) fetchConfig()
  }, [open])

  const isFeishuConfigured = !!(adapterConfig.feishu?.appId && adapterConfig.feishu?.appSecret
    && ((adapterConfig.feishu?.pairedUsers?.length ?? 0) > 0 || (adapterConfig.feishu?.allowedUsers?.length ?? 0) > 0))
  const isTelegramConfigured = !!(adapterConfig.telegram?.botToken
    && ((adapterConfig.telegram?.pairedUsers?.length ?? 0) > 0 || (adapterConfig.telegram?.allowedUsers?.length ?? 0) > 0))

  const isEdit = !!editTask
  const parsed = editTask ? parseCron(editTask.cron) : null

  const FREQUENCY_OPTIONS: Array<{ value: FrequencyKey; label: string }> = [
    { value: 'everyNMinutes', label: t('newTask.everyNMinutes') },
    { value: 'everyNHours', label: t('newTask.everyNHours') },
    { value: 'daily', label: t('newTask.daily') },
    { value: 'weekdays', label: t('newTask.weekdays') },
    { value: 'specificDays', label: t('newTask.specificDays') },
    { value: 'monthly', label: t('newTask.monthly') },
    { value: 'customCron', label: t('newTask.customCron') },
  ]

  const [name, setName] = useState(editTask?.name || '')
  const [description, setDescription] = useState(editTask?.description || '')
  const [prompt, setPrompt] = useState(editTask?.prompt || '')
  const [frequency, setFrequency] = useState<FrequencyKey>(parsed?.frequency || 'daily')
  const [time, setTime] = useState(parsed?.time || '09:00')
  const [model, setModel] = useState(editTask?.model || '')
  const [permissionMode, setPermissionMode] = useState<PermissionMode>((editTask?.permissionMode as PermissionMode) || 'askEveryTime')
  const [codingMode, setCodingMode] = useState<CodingModeId>((editTask?.codingMode as CodingModeId) ?? DEFAULT_CODING_MODE)
  const [folderPath, setFolderPath] = useState(editTask?.folderPath || defaultWorkDir)
  const [useWorktree, setUseWorktree] = useState(editTask?.useWorktree || false)
  const [notifyEnabled, setNotifyEnabled] = useState(editTask?.notification?.enabled || false)
  const [notifyChannels, setNotifyChannels] = useState<('telegram' | 'feishu')[]>(editTask?.notification?.channels || [])
  const [isSubmitting, setIsSubmitting] = useState(false)

  const initialTrigger: TriggerKind =
    editTask?.triggerType === 'idle'
      ? 'idle'
      : editTask?.triggerType === 'session_end'
        ? 'sessionEnd'
        : 'schedule'
  const [triggerKind, setTriggerKind] = useState<TriggerKind>(initialTrigger)
  const [afterIdleMinutes, setAfterIdleMinutes] = useState(
    editTask?.afterIdleMs ? Math.max(1, Math.round(editTask.afterIdleMs / 60000)) : 30,
  )
  const [priority, setPriority] = useState<TaskPriority>(editTask?.priority ?? 'normal')
  const [maxDurationMinutes, setMaxDurationMinutes] = useState(
    editTask?.maxDurationMs ? Math.max(0, Math.round(editTask.maxDurationMs / 60000)) : 0,
  )
  const [requireIdle, setRequireIdle] = useState(!!editTask?.requireIdleMs)
  const [requireIdleMinutes, setRequireIdleMinutes] = useState(
    editTask?.requireIdleMs ? Math.max(1, Math.round(editTask.requireIdleMs / 60000)) : 10,
  )

  const [minuteInterval, setMinuteInterval] = useState(parsed?.minuteInterval || 15)
  const [hourInterval, setHourInterval] = useState(parsed?.hourInterval || 1)
  const [minuteOffset, setMinuteOffset] = useState(parsed?.minuteOffset || 0)
  const [selectedDays, setSelectedDays] = useState<number[]>(parsed?.selectedDays || [1])
  const [monthDay, setMonthDay] = useState(parsed?.monthDay || 1)
  const [customCron, setCustomCron] = useState(parsed?.customCron || '0 9 * * *')

  const showTime = ['daily', 'weekdays', 'specificDays', 'monthly'].includes(frequency)

  const cronValue = buildCron(frequency, time, {
    minuteInterval, hourInterval, minuteOffset, selectedDays, monthDay, customCron,
  })

  const scheduleValid =
    (frequency !== 'customCron' || isValidCron(customCron)) &&
    (frequency !== 'specificDays' || selectedDays.length > 0)

  const canSubmit =
    !!name.trim() &&
    !!description.trim() &&
    !!prompt.trim() &&
    (triggerKind === 'schedule' ? scheduleValid : triggerKind === 'idle' ? afterIdleMinutes > 0 : true)

  const handleSubmit = async () => {
    if (!canSubmit) return
    setIsSubmitting(true)
    try {
      const base = {
        name: name.trim(),
        description: description.trim(),
        prompt: prompt.trim(),
        model: model || undefined,
        permissionMode,
        codingMode,
        folderPath: folderPath.trim() || undefined,
        useWorktree: useWorktree || undefined,
        notification: notifyEnabled && notifyChannels.length > 0
          ? { enabled: true as const, channels: notifyChannels }
          : undefined,
        priority,
        maxDurationMs: maxDurationMinutes > 0 ? maxDurationMinutes * 60000 : undefined,
      }

      const trigger =
        triggerKind === 'idle'
          ? {
              triggerType: 'idle' as const,
              afterIdleMs: afterIdleMinutes * 60000,
            }
          : triggerKind === 'sessionEnd'
            ? { triggerType: 'session_end' as const }
            : {
                triggerType: 'cron' as const,
                cron: cronValue,
                requireIdleMs:
                  requireIdle && requireIdleMinutes > 0 ? requireIdleMinutes * 60000 : undefined,
              }

      const payload = { ...base, ...trigger }
      if (isEdit) {
        await updateTask(editTask!.id, payload)
      } else {
        await createTask({ ...payload, enabled: true, recurring: triggerKind !== 'sessionEnd' })
      }
      onClose()
    } catch (err) {
      console.error(`Failed to ${isEdit ? 'update' : 'create'} task:`, err)
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={isEdit ? t('tasks.editTitle') : t('newTask.title')}
      titleClassName="text-xs font-semibold text-[var(--color-text-primary)]"
      compact
      footer={
        <>
          <Button size="sm" variant="secondary" onClick={onClose}>{t('common.cancel')}</Button>
          <Button size="sm" onClick={handleSubmit} disabled={!canSubmit} loading={isSubmitting}>
            {isEdit ? t('tasks.saveChanges') : t('newTask.create')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <SettingsSection title={t('newTask.sectionContent')} description={t('newTask.localWarning')}>
          <Input
            size="sm"
            label={t('newTask.name')}
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('newTask.namePlaceholder')}
          />

          <Input
            size="sm"
            label={t('newTask.description')}
            required
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t('newTask.descPlaceholder')}
          />

          <Field label={t('newTask.prompt')} required>
            <PromptEditor
              value={prompt}
              onChange={setPrompt}
              placeholder={t('newTask.promptPlaceholder')}
              codingMode={codingMode}
              onCodingModeChange={setCodingMode}
              permissionMode={permissionMode}
              onPermissionModeChange={setPermissionMode}
              modelId={model}
              onModelChange={setModel}
              folderPath={folderPath}
              onFolderPathChange={setFolderPath}
              useWorktree={useWorktree}
              onUseWorktreeChange={setUseWorktree}
            />
          </Field>
        </SettingsSection>

        <SettingsSection title={t('newTask.sectionTrigger')}>
          <Field label={t('automations.trigger.label')}>
            <SegmentedOption
              value={triggerKind}
              onChange={setTriggerKind}
              options={[
                { value: 'schedule', label: t('automations.trigger.schedule') },
                { value: 'idle', label: t('automations.trigger.idle') },
                { value: 'sessionEnd', label: t('automations.trigger.sessionEnd') },
              ]}
            />
          </Field>

          {triggerKind === 'idle' && (
            <Field label={t('automations.trigger.afterIdle')} hint={t('automations.trigger.idleHint')}>
              <input
                type="number"
                min={1}
                value={afterIdleMinutes}
                onChange={(e) => setAfterIdleMinutes(Math.max(1, Number(e.target.value) || 1))}
                className={FIELD_CLASS}
                style={{ maxWidth: 160 }}
              />
            </Field>
          )}

          {triggerKind === 'sessionEnd' && (
            <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-xs text-[var(--color-text-tertiary)]">
              <span className="material-symbols-outlined text-[16px]">bedtime</span>
              <span>{t('automations.trigger.sessionEndHint')}</span>
            </div>
          )}

          {triggerKind === 'schedule' && (
            <>
              <Field label={t('newTask.frequency')}>
                <NativeSelect
                  value={frequency}
                  onChange={(e) => setFrequency(e.target.value as FrequencyKey)}
                >
                  {FREQUENCY_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>{opt.label}</option>
                  ))}
                </NativeSelect>
              </Field>

              {frequency === 'everyNMinutes' && (
                <NativeSelect
                  value={minuteInterval}
                  onChange={(e) => setMinuteInterval(Number(e.target.value))}
                >
                  {MINUTE_INTERVALS.map((n) => (
                    <option key={n} value={n}>{t('newTask.intervalMinutes', { n })}</option>
                  ))}
                </NativeSelect>
              )}

              {frequency === 'everyNHours' && (
                <div className="flex gap-2">
                  <NativeSelect
                    className="flex-1"
                    value={hourInterval}
                    onChange={(e) => setHourInterval(Number(e.target.value))}
                  >
                    {HOUR_INTERVALS.map((n) => (
                      <option key={n} value={n}>{t('newTask.intervalHours', { n })}</option>
                    ))}
                  </NativeSelect>
                  <NativeSelect
                    className="flex-1"
                    value={minuteOffset}
                    onChange={(e) => setMinuteOffset(Number(e.target.value))}
                  >
                    {MINUTE_OFFSETS.map((m) => (
                      <option key={m} value={m}>{t('newTask.atMinute', { m: m.toString().padStart(2, '0') })}</option>
                    ))}
                  </NativeSelect>
                </div>
              )}

              {frequency === 'specificDays' && (
                <Field label={t('newTask.specificDays')}>
                  <DayOfWeekPicker selected={selectedDays} onChange={setSelectedDays} />
                </Field>
              )}

              {frequency === 'monthly' && (
                <NativeSelect
                  value={monthDay}
                  onChange={(e) => setMonthDay(Number(e.target.value))}
                >
                  {Array.from({ length: 28 }, (_, i) => i + 1).map((d) => (
                    <option key={d} value={d}>{t('newTask.onMonthDay', { d })}</option>
                  ))}
                </NativeSelect>
              )}

              {frequency === 'customCron' && (
                <Field hint={t('newTask.cronFormatHint')}>
                  <input
                    type="text"
                    value={customCron}
                    onChange={(e) => setCustomCron(e.target.value)}
                    placeholder={t('newTask.cronFormatHint')}
                    className={`${FIELD_CLASS} w-full font-[var(--font-mono)]`}
                  />
                  {customCron.trim() && !isValidCron(customCron) && (
                    <span className="text-xs text-[var(--color-error)]">{t('newTask.invalidCron')}</span>
                  )}
                </Field>
              )}

              {showTime && (
                <Field label={t('newTask.time')}>
                  <input
                    type="time"
                    value={time}
                    onChange={(e) => setTime(e.target.value)}
                    className={FIELD_CLASS}
                    style={{ maxWidth: 120 }}
                  />
                </Field>
              )}

              <div className="flex flex-col gap-2">
                <ToggleRow
                  label={t('automations.requireIdle')}
                  hint={t('automations.requireIdleHint')}
                  checked={requireIdle}
                  onChange={setRequireIdle}
                />
                {requireIdle && (
                  <div className="flex items-center gap-2 pl-1">
                    <input
                      type="number"
                      min={1}
                      value={requireIdleMinutes}
                      onChange={(e) => setRequireIdleMinutes(Math.max(1, Number(e.target.value) || 1))}
                      className={FIELD_CLASS}
                      style={{ maxWidth: 140 }}
                    />
                    <span className="text-xs text-[var(--color-text-tertiary)]">{t('automations.minutesUnit')}</span>
                  </div>
                )}
              </div>

              <p className="text-xs text-[var(--color-text-tertiary)]">
                {frequency === 'customCron' && customCron.trim() && !isValidCron(customCron)
                  ? t('newTask.invalidCron')
                  : describeCron(cronValue, t)}
              </p>
            </>
          )}
        </SettingsSection>

        <SettingsSection title={t('newTask.sectionRun')} description={t('newTask.delayNote')}>
          <Field label={t('automations.priority.label')}>
            <SegmentedOption
              value={priority}
              onChange={setPriority}
              options={[
                { value: 'high', label: t('automations.priority.high') },
                { value: 'normal', label: t('automations.priority.normal') },
                { value: 'low', label: t('automations.priority.low') },
              ]}
            />
          </Field>

          <Field label={t('automations.maxDuration')} hint={t('automations.maxDurationHint')}>
            <input
              type="number"
              min={0}
              value={maxDurationMinutes}
              onChange={(e) => setMaxDurationMinutes(Math.max(0, Number(e.target.value) || 0))}
              className={FIELD_CLASS}
              style={{ maxWidth: 160 }}
            />
          </Field>

          <div className="flex flex-col gap-2">
            <ToggleRow
              label={t('newTask.notifyOnComplete')}
              hint={t('newTask.notifyHint')}
              checked={notifyEnabled}
              onChange={setNotifyEnabled}
            />
            {notifyEnabled && (
              <div className="flex flex-col gap-2">
                <div className="flex flex-wrap items-center gap-2">
                  <label className={`flex items-center justify-between gap-2 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 ${isFeishuConfigured ? 'cursor-pointer' : 'cursor-not-allowed opacity-60'}`}>
                    <span className="text-xs text-[var(--color-text-primary)]">{t('settings.adapters.feishu')}</span>
                    <input
                      type="checkbox"
                      checked={notifyChannels.includes('feishu')}
                      disabled={!isFeishuConfigured}
                      onChange={(e) => {
                        setNotifyChannels((prev) =>
                          e.target.checked ? [...prev, 'feishu'] : prev.filter((c) => c !== 'feishu'),
                        )
                      }}
                      className="h-4 w-4 rounded border-[var(--color-border)] accent-[var(--color-brand)]"
                    />
                  </label>
                  {!isFeishuConfigured && (
                    <span className="text-xs text-[var(--color-warning)]">{t('newTask.notConfigured')}</span>
                  )}
                  <label className={`flex items-center justify-between gap-2 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 ${isTelegramConfigured ? 'cursor-pointer' : 'cursor-not-allowed opacity-60'}`}>
                    <span className="text-xs text-[var(--color-text-primary)]">{t('settings.adapters.telegram')}</span>
                    <input
                      type="checkbox"
                      checked={notifyChannels.includes('telegram')}
                      disabled={!isTelegramConfigured}
                      onChange={(e) => {
                        setNotifyChannels((prev) =>
                          e.target.checked ? [...prev, 'telegram'] : prev.filter((c) => c !== 'telegram'),
                        )
                      }}
                      className="h-4 w-4 rounded border-[var(--color-border)] accent-[var(--color-brand)]"
                    />
                  </label>
                  {!isTelegramConfigured && (
                    <span className="text-xs text-[var(--color-warning)]">{t('newTask.notConfigured')}</span>
                  )}
                </div>
                {!isFeishuConfigured && !isTelegramConfigured && (
                  <p className="text-xs text-[var(--color-warning)]">
                    <span className="material-symbols-outlined mr-1 align-middle text-[12px]">warning</span>
                    {t('newTask.noChannelConfigured')}
                  </p>
                )}
              </div>
            )}
          </div>
        </SettingsSection>
      </div>
    </Modal>
  )
}

function SegmentedOption<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T
  options: Array<{ value: T; label: string }>
  onChange: (value: T) => void
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          onClick={() => onChange(opt.value)}
          className={`h-7 min-w-[88px] rounded-lg border px-3 text-xs font-semibold transition-all ${
            value === opt.value
              ? 'border-[var(--color-brand)] bg-[var(--color-brand)] text-[var(--color-on-primary)]'
              : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
          }`}
        >
          {opt.label}
        </button>
      ))}
    </div>
  )
}

function ToggleRow({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string
  hint?: string
  checked: boolean
  onChange: (next: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] px-3 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="text-xs font-medium text-[var(--color-text-primary)]">{label}</div>
        {hint && (
          <div className="mt-0.5 text-xs text-[var(--color-text-tertiary)]">{hint}</div>
        )}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors ${
          checked ? 'bg-[var(--color-brand)]' : 'bg-[var(--color-surface-hover)]'
        }`}
      >
        <span
          className={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${
            checked ? 'translate-x-5' : 'translate-x-0.5'
          }`}
        />
      </button>
    </div>
  )
}
