import { useEffect, useState } from 'react'
import { useTranslation } from '../i18n'
import { Button } from '../components/shared/Button'
import { useSkillStore } from '../stores/skillStore'
import { useUIStore } from '../stores/uiStore'
import { SkillList } from '../components/skills/SkillList'
import { SkillDetail } from '../components/skills/SkillDetail'
import { RuleList } from '../components/rules/RuleList'
import { SubagentList } from '../components/subagents/SubagentList'
import { skillsApi } from '../api/skills'

type SubTab = 'rules' | 'skills' | 'subagents'

export function RulesSkillsSubagentsSettings() {
  const t = useTranslation()
  const [tab, setTab] = useState<SubTab>('rules')

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-[var(--color-text-primary)]">
          {t('settings.rsk.title')}
        </h2>
        <p className="text-xs text-[var(--color-text-secondary)] mt-1">
          {t('settings.rsk.description')}
        </p>
      </div>

      <div className="flex items-center gap-1 border-b border-[var(--color-border)]">
        <SubTabButton
          active={tab === 'rules'}
          onClick={() => setTab('rules')}
          icon="rule"
          label={t('settings.rsk.subtabRules')}
        />
        <SubTabButton
          active={tab === 'skills'}
          onClick={() => setTab('skills')}
          icon="auto_awesome"
          label={t('settings.rsk.subtabSkills')}
        />
        <SubTabButton
          active={tab === 'subagents'}
          onClick={() => setTab('subagents')}
          icon="smart_toy"
          label={t('settings.rsk.subtabSubagents')}
        />
      </div>

      {tab === 'rules' && <RuleList />}
      {tab === 'skills' && <SkillsTab />}
      {tab === 'subagents' && <SubagentList />}
    </div>
  )
}

function SkillsTab() {
  const t = useTranslation()
  const selectedSkill = useSkillStore((s) => s.selectedSkill)
  const setDisabledSkills = useSkillStore((s) => s.setDisabledSkills)
  const fetchSkills = useSkillStore((s) => s.fetchSkills)
  const addToast = useUIStore((s) => s.addToast)
  const [disabledText, setDisabledText] = useState('')
  const [hasLoadedDisabled, setHasLoadedDisabled] = useState(false)
  const [isSaving, setIsSaving] = useState(false)
  const [promptMode, setPromptMode] = useState<'full' | 'compact'>('compact')
  const [isSavingMode, setIsSavingMode] = useState(false)

  useEffect(() => {
    if (hasLoadedDisabled) return
    void (async () => {
      try {
        const raw = await skillsApi.list()
        const list = (raw as unknown as { disabled_skills?: string[] })
          ?.disabled_skills
        if (Array.isArray(list)) {
          setDisabledText(list.join(', '))
        }
        const mode = (raw as unknown as { prompt_injection_mode?: string })
          ?.prompt_injection_mode
        if (mode === 'full' || mode === 'compact') {
          setPromptMode(mode)
        }
      } catch {

      } finally {
        setHasLoadedDisabled(true)
      }
    })()
  }, [hasLoadedDisabled])

  async function handleChangePromptMode(next: 'full' | 'compact') {
    if (next === promptMode) return
    setIsSavingMode(true)
    try {
      await skillsApi.setPromptInjectionMode(next)
      setPromptMode(next)
      addToast({ type: 'success', message: t('settings.skills.promptModeSavedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setIsSavingMode(false)
    }
  }

  if (selectedSkill) {
    return (
      <div className="w-full min-w-0">
        <SkillDetail />
      </div>
    )
  }

  async function handleSaveDisabled() {
    setIsSaving(true)
    try {
      const list = disabledText
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
      await setDisabledSkills(list)
      addToast({ type: 'success', message: t('settings.skills.disabledSavedToast') })
      void fetchSkills()
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <div className="space-y-4">
      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-4 space-y-3">
        <div>
          <p className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('settings.skills.promptModeTitle')}
          </p>
          <p className="text-[11px] text-[var(--color-text-tertiary)] leading-relaxed">
            {t('settings.skills.promptModeHint')}
          </p>
        </div>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
          <PromptModeOption
            active={promptMode === 'compact'}
            disabled={isSavingMode}
            title={t('settings.skills.promptMode.compactTitle')}
            description={t('settings.skills.promptMode.compactDesc')}
            badge={t('settings.skills.promptMode.recommended')}
            onClick={() => handleChangePromptMode('compact')}
          />
          <PromptModeOption
            active={promptMode === 'full'}
            disabled={isSavingMode}
            title={t('settings.skills.promptMode.fullTitle')}
            description={t('settings.skills.promptMode.fullDesc')}
            onClick={() => handleChangePromptMode('full')}
          />
        </div>
      </section>

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-4 space-y-2">
        <div>
          <p className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('settings.skills.disabledTitle')}
          </p>
          <p className="text-[11px] text-[var(--color-text-tertiary)]">
            {t('settings.skills.disabledHint')}
          </p>
        </div>
        <input
          className="w-full h-9 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-xs"
          value={disabledText}
          onChange={(e) => setDisabledText(e.target.value)}
          placeholder="skillA, skillB"
        />
        <div className="flex justify-end">
          <Button onClick={handleSaveDisabled} disabled={isSaving}>
            {isSaving ? t('common.saving') : t('common.save')}
          </Button>
        </div>
      </section>

      <div className="w-full min-w-0">
        <SkillList />
      </div>
    </div>
  )
}

function PromptModeOption({
  active,
  disabled,
  title,
  description,
  badge,
  onClick,
}: {
  active: boolean
  disabled: boolean
  title: string
  description: string
  badge?: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`text-left rounded-md border p-3 transition-colors flex flex-col gap-1.5 ${
        active
          ? 'border-[var(--color-primary)] bg-[var(--color-surface)] ring-1 ring-[var(--color-primary)]'
          : 'border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-text-tertiary)]'
      } ${disabled ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer'}`}
    >
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-[var(--color-text-primary)]">{title}</span>
        {badge && (
          <span className="text-[10px] font-semibold uppercase tracking-wide rounded px-1.5 py-[1px] bg-[var(--color-primary)] text-[var(--color-on-primary)]">
            {badge}
          </span>
        )}
      </div>
      <span className="text-[11px] text-[var(--color-text-tertiary)] leading-relaxed">
        {description}
      </span>
    </button>
  )
}

function SubTabButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean
  onClick: () => void
  icon: string
  label: string
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-1.5 px-3 py-2 text-xs transition-colors border-b-2 -mb-[1px] ${
        active
          ? 'border-[var(--color-brand)] text-[var(--color-text-primary)] font-medium'
          : 'border-transparent text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]'
      }`}
    >
      <span className="material-symbols-outlined text-[14px]">{icon}</span>
      {label}
    </button>
  )
}
