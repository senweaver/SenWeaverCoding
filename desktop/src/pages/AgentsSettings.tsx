import { useEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import DOMPurify from 'dompurify'
import { useTranslation, type TranslationKey } from '../i18n'
import { Button } from '../components/shared/Button'
import { Input } from '../components/shared/Input'
import { useAgentSettingsStore } from '../stores/agentSettingsStore'
import { useAutonomyStore } from '../stores/autonomyStore'
import { useSettingsStore } from '../stores/settingsStore'
import { useUIStore } from '../stores/uiStore'
import type { GlobalDirective, ThinkingLevel } from '../types/agentSettings'
import type { AutonomySettings, PermissionMode } from '../types/settings'

const THINKING_LEVELS: ThinkingLevel[] = ['off', 'minimal', 'low', 'medium', 'high', 'max']
const TOOL_DISPATCHERS = ['auto', 'sequential', 'parallel']
const SEARCH_PROVIDERS = ['duckduckgo', 'brave', 'searxng', 'tavily', 'exa']

function splitList(raw: string): string[] {
  return raw
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

export function AgentsSettings() {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)
  const agentConfig = useAgentSettingsStore((s) => s.agentConfig)
  const agentRuntime = useAgentSettingsStore((s) => s.agentRuntime)
  const webSearch = useAgentSettingsStore((s) => s.webSearch)
  const webFetch = useAgentSettingsStore((s) => s.webFetch)
  const isLoading = useAgentSettingsStore((s) => s.isLoading)
  const isSaving = useAgentSettingsStore((s) => s.isSaving)
  const error = useAgentSettingsStore((s) => s.error)
  const fetchAll = useAgentSettingsStore((s) => s.fetchAll)
  const updateAgent = useAgentSettingsStore((s) => s.updateAgent)
  const updateRuntime = useAgentSettingsStore((s) => s.updateRuntime)
  const updateWebSearch = useAgentSettingsStore((s) => s.updateWebSearch)
  const updateWebFetch = useAgentSettingsStore((s) => s.updateWebFetch)

  useEffect(() => {
    void fetchAll()
  }, [fetchAll])

  return (
    <div className="max-w-3xl space-y-6">
      <div>
        <h2 className="text-lg font-semibold text-[var(--color-text-primary)]">
          {t('settings.agents.title')}
        </h2>
        <p className="text-xs text-[var(--color-text-secondary)] mt-1">
          {t('settings.agents.description')}
        </p>
      </div>

      <AutoRunSection />

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      {isLoading && !agentConfig && (
        <div className="text-xs text-[var(--color-text-secondary)]">…</div>
      )}

      {agentConfig && (
        <CoreSection
          onSave={async (patch) => {
            await updateAgent(patch)
            addToast({ type: 'success', message: t('settings.agents.savedToast') })
          }}
          isSaving={isSaving}
        />
      )}

      {agentConfig && (
        <ContextSection
          onSave={async (patch) => {
            await updateAgent(patch)
            addToast({ type: 'success', message: t('settings.agents.savedToast') })
          }}
          isSaving={isSaving}
        />
      )}

      {agentConfig && (
        <QualitySection
          onSave={async (patch) => {
            await updateAgent(patch)
            addToast({ type: 'success', message: t('settings.agents.savedToast') })
          }}
          isSaving={isSaving}
        />
      )}

      {agentConfig && (
        <DirectivesSection
          onSave={async (patch) => {
            await updateAgent(patch)
            addToast({ type: 'success', message: t('settings.agents.savedToast') })
          }}
          isSaving={isSaving}
        />
      )}

      {agentRuntime && (
        <RuntimeSection
          onSave={async (patch) => {
            await updateRuntime(patch)
            addToast({ type: 'success', message: t('settings.agents.savedToast') })
          }}
          isSaving={isSaving}
        />
      )}

      {(webSearch || webFetch) && (
        <ContextToolsSection
          onSaveSearch={async (patch) => {
            await updateWebSearch(patch)
            addToast({ type: 'success', message: t('settings.agents.savedToast') })
          }}
          onSaveFetch={async (patch) => {
            await updateWebFetch(patch)
            addToast({ type: 'success', message: t('settings.agents.savedToast') })
          }}
          isSaving={isSaving}
        />
      )}
    </div>
  )
}

type AutoRunOption = {
  value: PermissionMode
  labelKey: TranslationKey
  hintKey: TranslationKey
  icon: string
  danger?: boolean
}

const AUTO_RUN_OPTIONS: AutoRunOption[] = [
  {
    value: 'askEveryTime',
    labelKey: 'settings.agents.autoRun.opt.askEveryTime',
    hintKey: 'settings.agents.autoRun.opt.askEveryTimeHint',
    icon: 'help',
  },
  {
    value: 'acceptEdits',
    labelKey: 'settings.agents.autoRun.opt.acceptEdits',
    hintKey: 'settings.agents.autoRun.opt.acceptEditsHint',
    icon: 'bolt',
  },
  {
    value: 'default',
    labelKey: 'settings.agents.autoRun.opt.useAllowlist',
    hintKey: 'settings.agents.autoRun.opt.useAllowlistHint',
    icon: 'rule',
  },
  {
    value: 'bypassPermissions',
    labelKey: 'settings.agents.autoRun.opt.runEverything',
    hintKey: 'settings.agents.autoRun.opt.runEverythingHint',
    icon: 'gavel',
    danger: true,
  },
]

function AllowlistEditor({
  data,
  onPatch,
  isSaving,
}: {
  data: AutonomySettings
  onPatch: (patch: Partial<AutonomySettings>) => Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  const [draft, setDraft] = useState('')
  const list = data.autoApprove

  const add = async () => {
    const trimmed = draft.trim()
    if (!trimmed) return
    if (list.includes(trimmed)) {
      setDraft('')
      return
    }
    await onPatch({ autoApprove: [...list, trimmed] })
    setDraft('')
  }

  const remove = async (name: string) => {
    await onPatch({ autoApprove: list.filter((x) => x !== name) })
  }

  return (
    <div className="mt-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)] p-3">
      <div className="mb-1 flex items-center justify-between">
        <div className="text-[12px] font-semibold text-[var(--color-text-primary)]">
          {t('settings.agents.autoRun.allowlist.title')}
        </div>
      </div>
      <p className="mb-2 text-[11px] leading-snug text-[var(--color-text-tertiary)]">
        {t('settings.agents.autoRun.allowlist.hint')}
      </p>
      <div className="flex items-center gap-2">
        <Input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              void add()
            }
          }}
          placeholder={t('settings.agents.autoRun.allowlist.placeholder')}
          className="h-8 flex-1 text-[12px]"
          disabled={isSaving}
        />
        <Button
          variant="primary"
          onClick={() => void add()}
          disabled={isSaving || !draft.trim()}
          className="h-8 text-[12px]"
        >
          {t('settings.agents.autoRun.allowlist.add')}
        </Button>
      </div>
      {list.length === 0 ? (
        <div className="mt-2 text-[11px] text-[var(--color-text-tertiary)]">
          {t('settings.agents.autoRun.allowlist.empty')}
        </div>
      ) : (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {list.map((name) => (
            <span
              key={name}
              className="inline-flex items-center gap-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] px-2 py-0.5 text-[11px] text-[var(--color-text-primary)]"
            >
              <code className="font-mono">{name}</code>
              <button
                type="button"
                aria-label={t('settings.agents.autoRun.allowlist.remove')}
                onClick={() => void remove(name)}
                disabled={isSaving}
                className="text-[var(--color-text-tertiary)] hover:text-[var(--color-error)]"
              >
                <span className="material-symbols-outlined text-[14px]">close</span>
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

function ToggleRow({
  label,
  hint,
  checked,
  onChange,
  disabled,
}: {
  label: string
  hint: string
  checked: boolean
  onChange: (next: boolean) => void
  disabled?: boolean
}) {
  return (
    <div className="flex items-start justify-between gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
          {label}
        </div>
        <div className="mt-0.5 text-[11px] leading-snug text-[var(--color-text-tertiary)]">
          {hint}
        </div>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`mt-0.5 inline-flex h-5 w-9 flex-shrink-0 items-center rounded-full transition-colors ${
          checked ? 'bg-[var(--color-brand)]' : 'bg-[var(--color-outline)]'
        } ${disabled ? 'opacity-60' : ''}`}
      >
        <span
          className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
            checked ? 'translate-x-4' : 'translate-x-0.5'
          }`}
        />
      </button>
    </div>
  )
}

function AutoRunSection() {
  const t = useTranslation()
  const permissionMode = useSettingsStore((s) => s.permissionMode)
  const setPermissionMode = useSettingsStore((s) => s.setPermissionMode)
  const codingMode = useSettingsStore((s) => s.codingMode)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const addToast = useUIStore((s) => s.addToast)

  const data = useAutonomyStore((s) => s.data)
  const isLoading = useAutonomyStore((s) => s.isLoading)
  const isSaving = useAutonomyStore((s) => s.isSaving)
  const fetchAutonomy = useAutonomyStore((s) => s.fetch)
  const updatePartial = useAutonomyStore((s) => s.updatePartial)
  const hasFetched = useAutonomyStore((s) => s.hasFetched)

  const [confirmBypass, setConfirmBypass] = useState(false)

  useEffect(() => {
    if (!hasFetched && !isLoading) {
      void fetchAutonomy()
    }
  }, [hasFetched, isLoading, fetchAutonomy])

  const PERMISSION_LABELS: Record<PermissionMode, string> = {
    default: t('permMode.label.default'),
    acceptEdits: t('permMode.label.acceptEdits'),
    plan: t('permMode.label.plan'),
    bypassPermissions: t('permMode.label.bypassPermissions'),
    dontAsk: t('permMode.label.dontAsk'),
    askEveryTime: t('settings.agents.autoRun.opt.askEveryTime'),
  }

  const currentCoding = codingModes.find((m) => m.id === codingMode)
  const derivedLabel = currentCoding ? PERMISSION_LABELS[currentCoding.permissionMode] : null

  const applyMode = async (mode: PermissionMode) => {
    try {
      await setPermissionMode(mode)
      addToast({ type: 'success', message: t('settings.agents.savedToast') })
    } catch (e) {
      addToast({
        type: 'error',
        message: e instanceof Error ? e.message : String(e),
      })
    }
  }

  const onSelectMode = (mode: PermissionMode) => {
    if (mode === 'bypassPermissions' && permissionMode !== 'bypassPermissions') {
      setConfirmBypass(true)
      return
    }
    void applyMode(mode)
  }

  const onAutonomyPatch = async (patch: Partial<AutonomySettings>) => {
    try {
      await updatePartial(patch)
    } catch (e) {
      addToast({
        type: 'error',
        message: e instanceof Error ? e.message : String(e),
      })
    }
  }

  const uiSelectedValue: PermissionMode =
    permissionMode === 'plan' ? 'default' : (permissionMode as PermissionMode)

  const enabledTransitions = new Set(data?.autoApproveModeTransitions ?? [])

  return (
    <Section
      title={t('settings.agents.section.permission')}
      hint={t('settings.agents.section.permissionHint')}
    >
      <div className="mb-1 flex items-center justify-between">
        <label htmlFor="auto-run-mode" className="text-[12px] font-semibold text-[var(--color-text-primary)]">
          {t('settings.agents.autoRun.modeLabel')}
        </label>
      </div>
      <p className="mb-2 text-[11px] leading-snug text-[var(--color-text-tertiary)]">
        {t('settings.agents.autoRun.modeHint')}
      </p>
      <select
        id="auto-run-mode"
        value={uiSelectedValue}
        onChange={(e) => onSelectMode(e.target.value as PermissionMode)}
        className="w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-[13px] text-[var(--color-text-primary)] focus:border-[var(--color-brand)] focus:outline-none"
      >
        {AUTO_RUN_OPTIONS.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {t(opt.labelKey)}
          </option>
        ))}
      </select>
      <p className="mt-1 text-[11px] leading-snug text-[var(--color-text-tertiary)]">
        {t(
          AUTO_RUN_OPTIONS.find((o) => o.value === uiSelectedValue)?.hintKey
            ?? 'settings.agents.autoRun.opt.useAllowlistHint',
        )}
      </p>

      {permissionMode === 'default' && data && (
        <AllowlistEditor data={data} onPatch={onAutonomyPatch} isSaving={isSaving} />
      )}

      {data && (
        <>
          <div className="mt-4">
            <div className="mb-1 text-[12px] font-semibold text-[var(--color-text-primary)]">
              {t('settings.agents.autoRun.modeTransitions.title')}
            </div>
            <p className="mb-2 text-[11px] leading-snug text-[var(--color-text-tertiary)]">
              {t('settings.agents.autoRun.modeTransitions.hint')}
            </p>
            <div className="space-y-1.5">
              {codingModes.map((m) => {
                const checked = enabledTransitions.has(m.id)
                return (
                  <ToggleRow
                    key={m.id}
                    label={m.label}
                    hint={m.description ?? ''}
                    checked={checked}
                    onChange={(next) => {
                      const nextList = next
                        ? [...(data.autoApproveModeTransitions ?? []), m.id]
                        : (data.autoApproveModeTransitions ?? []).filter((x) => x !== m.id)
                      void onAutonomyPatch({ autoApproveModeTransitions: nextList })
                    }}
                    disabled={isSaving}
                  />
                )
              })}
            </div>
          </div>

          <div className="mt-4 space-y-1.5">
            <ToggleRow
              label={t('settings.agents.autoRun.browserProtection')}
              hint={t('settings.agents.autoRun.browserProtectionHint')}
              checked={data.protectBrowserTools}
              onChange={(next) => void onAutonomyPatch({ protectBrowserTools: next })}
              disabled={isSaving}
            />
            <ToggleRow
              label={t('settings.agents.autoRun.mcpProtection')}
              hint={t('settings.agents.autoRun.mcpProtectionHint')}
              checked={data.protectMcpTools}
              onChange={(next) => void onAutonomyPatch({ protectMcpTools: next })}
              disabled={isSaving}
            />
          </div>
        </>
      )}

      {currentCoding && derivedLabel && (
        <div className="mt-3 flex items-start gap-1.5 text-[11px] text-[var(--color-text-tertiary)]">
          <span className="material-symbols-outlined text-[14px]">info</span>
          <span>
            {t('settings.agents.permission.codingHint', {
              mode: currentCoding.label,
              derived: derivedLabel,
            })}
          </span>
        </div>
      )}

      {confirmBypass &&
        createPortal(
          <div
            className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 pl-[var(--sidebar-width)]"
            role="presentation"
            onClick={() => setConfirmBypass(false)}
          >
            <div
              className="w-[420px] overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]"
              role="dialog"
              aria-modal="true"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="flex items-center gap-3 border-b border-[var(--color-error)]/15 bg-[var(--color-error)]/8 px-5 py-4">
                <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--color-error)]/12">
                  <span className="material-symbols-outlined text-[22px] text-[var(--color-error)]">
                    warning
                  </span>
                </div>
                <div>
                  <div className="text-sm font-bold text-[var(--color-text-primary)]">
                    {t('permMode.enableBypassTitle')}
                  </div>
                  <div className="mt-0.5 text-xs text-[var(--color-text-tertiary)]">
                    {t('permMode.enableBypassSubtitle')}
                  </div>
                </div>
              </div>

              <div className="px-5 py-4">
                <p
                  className="mb-3 text-xs leading-relaxed text-[var(--color-text-secondary)]"
                  dangerouslySetInnerHTML={{
                    __html: DOMPurify.sanitize(t('permMode.enableBypassBody')),
                  }}
                />
                <ul className="mt-3 space-y-1.5 text-xs text-[var(--color-text-secondary)]">
                  <li className="flex items-start gap-2">
                    <span className="material-symbols-outlined mt-0.5 text-[14px] text-[var(--color-error)]">
                      check
                    </span>
                    {t('permMode.permReadWrite')}
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="material-symbols-outlined mt-0.5 text-[14px] text-[var(--color-error)]">
                      check
                    </span>
                    {t('permMode.permShell')}
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="material-symbols-outlined mt-0.5 text-[14px] text-[var(--color-error)]">
                      check
                    </span>
                    {t('permMode.permPackages')}
                  </li>
                </ul>
              </div>

              <div className="flex items-center justify-end gap-2 border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-5 py-3">
                <button
                  type="button"
                  onClick={() => setConfirmBypass(false)}
                  className="rounded-lg px-4 py-2 text-xs font-semibold text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
                >
                  {t('common.cancel')}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setConfirmBypass(false)
                    void applyMode('bypassPermissions')
                  }}
                  className="rounded-lg bg-[var(--color-error)] px-4 py-2 text-xs font-semibold text-white transition-colors hover:opacity-90"
                >
                  {t('permMode.enableBypassBtn')}
                </button>
              </div>
            </div>
          </div>,
          document.body,
        )}
    </Section>
  )
}

function CoreSection({
  onSave,
  isSaving,
}: {
  onSave: (patch: Record<string, unknown>) => Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  const cfg = useAgentSettingsStore((s) => s.agentConfig)

  const [compact, setCompact] = useState(false)
  const [parallel, setParallel] = useState(false)
  const [contextAware, setContextAware] = useState(false)
  const [maxIters, setMaxIters] = useState(0)
  const [maxHistory, setMaxHistory] = useState(0)
  const [maxCtx, setMaxCtx] = useState(0)
  const [dispatcher, setDispatcher] = useState('auto')

  useEffect(() => {
    if (!cfg) return
    setCompact(cfg.compactContext)
    setParallel(cfg.parallelTools)
    setContextAware(cfg.contextAwareTools)
    setMaxIters(cfg.maxToolIterations)
    setMaxHistory(cfg.maxHistoryMessages)
    setMaxCtx(cfg.maxContextTokens)
    setDispatcher(cfg.toolDispatcher || 'auto')
  }, [cfg])

  async function save() {
    try {
      await onSave({
        compactContext: compact,
        parallelTools: parallel,
        contextAwareTools: contextAware,
        maxToolIterations: maxIters,
        maxHistoryMessages: maxHistory,
        maxContextTokens: maxCtx,
        toolDispatcher: dispatcher,
      })
    } catch {

    }
  }

  return (
    <Section
      title={t('settings.agents.section.core')}
      hint={t('settings.agents.section.coreHint')}
    >
      <CheckboxRow
        checked={compact}
        onChange={setCompact}
        label={t('settings.agents.field.compactContext')}
        hint={t('settings.agents.field.compactContextHint')}
      />
      <CheckboxRow
        checked={parallel}
        onChange={setParallel}
        label={t('settings.agents.field.parallelTools')}
        hint={t('settings.agents.field.parallelToolsHint')}
      />
      <CheckboxRow
        checked={contextAware}
        onChange={setContextAware}
        label={t('settings.agents.field.contextAwareTools')}
        hint={t('settings.agents.field.contextAwareToolsHint')}
      />
      <Field
        label={t('settings.agents.field.toolDispatcher')}
        hint={t('settings.agents.field.toolDispatcherHint')}
      >
        <Select value={dispatcher} onChange={setDispatcher} options={TOOL_DISPATCHERS} />
      </Field>
      <NumberField
        label={t('settings.agents.field.maxToolIterations')}
        hint={t('settings.agents.field.maxToolIterationsHint')}
        value={maxIters}
        onChange={setMaxIters}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.maxHistoryMessages')}
        hint={t('settings.agents.field.maxHistoryMessagesHint')}
        value={maxHistory}
        onChange={setMaxHistory}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.maxContextTokens')}
        hint={t('settings.agents.field.maxContextTokensHint')}
        value={maxCtx}
        onChange={setMaxCtx}
        min={0}
      />
      <SaveBar onSave={save} isSaving={isSaving} />
    </Section>
  )
}

function ContextSection({
  onSave,
  isSaving,
}: {
  onSave: (patch: Record<string, unknown>) => Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  const cfg = useAgentSettingsStore((s) => s.agentConfig)

  const [hpEnabled, setHpEnabled] = useState(false)
  const [hpMaxTokens, setHpMaxTokens] = useState(0)
  const [hpKeepRecent, setHpKeepRecent] = useState(0)
  const [hpCollapse, setHpCollapse] = useState(false)

  const [ccEnabled, setCcEnabled] = useState(false)
  const [ccThreshold, setCcThreshold] = useState(0.7)
  const [ccFirstN, setCcFirstN] = useState(0)
  const [ccLastN, setCcLastN] = useState(0)
  const [ccMaxPasses, setCcMaxPasses] = useState(1)
  const [ccSummaryChars, setCcSummaryChars] = useState(0)
  const [ccSourceChars, setCcSourceChars] = useState(0)
  const [ccTimeout, setCcTimeout] = useState(0)
  const [ccSummaryModel, setCcSummaryModel] = useState('')

  const [bdEnabled, setBdEnabled] = useState(false)

  useEffect(() => {
    if (!cfg) return
    const hp = cfg.historyPruning
    setHpEnabled(hp.enabled)
    setHpMaxTokens(hp.maxTokens)
    setHpKeepRecent(hp.keepRecent)
    setHpCollapse(hp.collapseToolResults)

    const cc = cfg.contextCompression
    setCcEnabled(cc.enabled)
    setCcThreshold(cc.thresholdRatio)
    setCcFirstN(cc.protectFirstN)
    setCcLastN(cc.protectLastN)
    setCcMaxPasses(cc.maxPasses)
    setCcSummaryChars(cc.summaryMaxChars)
    setCcSourceChars(cc.sourceMaxChars)
    setCcTimeout(cc.timeoutSecs)
    setCcSummaryModel(cc.summaryModel ?? '')

    setBdEnabled(cfg.builtinToolDeferredLoading ?? false)
  }, [cfg])

  async function save() {
    try {
      await onSave({
        historyPruning: {
          enabled: hpEnabled,
          maxTokens: hpMaxTokens,
          keepRecent: hpKeepRecent,
          collapseToolResults: hpCollapse,
        },
        contextCompression: {
          enabled: ccEnabled,
          thresholdRatio: ccThreshold,
          protectFirstN: ccFirstN,
          protectLastN: ccLastN,
          maxPasses: ccMaxPasses,
          summaryMaxChars: ccSummaryChars,
          sourceMaxChars: ccSourceChars,
          timeoutSecs: ccTimeout,
          summaryModel: ccSummaryModel.trim().length > 0 ? ccSummaryModel.trim() : null,
        },
        builtinToolDeferredLoading: bdEnabled,
      })
    } catch {

    }
  }

  return (
    <Section
      title={t('settings.agents.section.context')}
      hint={t('settings.agents.section.contextHint')}
    >
      <CheckboxRow
        checked={hpEnabled}
        onChange={setHpEnabled}
        label={t('settings.agents.field.historyEnabled')}
      />
      <NumberField
        label={t('settings.agents.field.historyMaxTokens')}
        value={hpMaxTokens}
        onChange={setHpMaxTokens}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.historyKeepRecent')}
        value={hpKeepRecent}
        onChange={setHpKeepRecent}
        min={0}
      />
      <CheckboxRow
        checked={hpCollapse}
        onChange={setHpCollapse}
        label={t('settings.agents.field.historyCollapseTools')}
      />

      <div className="border-t border-[var(--color-border)] my-2" />

      <CheckboxRow
        checked={ccEnabled}
        onChange={setCcEnabled}
        label={t('settings.agents.field.compressionEnabled')}
      />
      <Field
        label={t('settings.agents.field.compressionThreshold')}
        hint={t('settings.agents.field.compressionThresholdHint')}
      >
        <Input
          type="number"
          step={0.05}
          min={0}
          max={1}
          value={ccThreshold}
          onChange={(e) => setCcThreshold(Number.parseFloat(e.target.value || '0'))}
        />
      </Field>
      <NumberField
        label={t('settings.agents.field.compressionFirstN')}
        value={ccFirstN}
        onChange={setCcFirstN}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.compressionLastN')}
        value={ccLastN}
        onChange={setCcLastN}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.compressionMaxPasses')}
        value={ccMaxPasses}
        onChange={setCcMaxPasses}
        min={1}
      />
      <NumberField
        label={t('settings.agents.field.compressionSummaryChars')}
        value={ccSummaryChars}
        onChange={setCcSummaryChars}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.compressionSourceChars')}
        value={ccSourceChars}
        onChange={setCcSourceChars}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.compressionTimeout')}
        value={ccTimeout}
        onChange={setCcTimeout}
        min={0}
      />
      <Field
        label={t('settings.agents.field.compressionSummaryModel')}
        hint={t('settings.agents.field.compressionSummaryModelHint')}
      >
        <Input value={ccSummaryModel} onChange={(e) => setCcSummaryModel(e.target.value)} />
      </Field>

      <div className="border-t border-[var(--color-border)] my-2" />

      <CheckboxRow
        checked={bdEnabled}
        onChange={setBdEnabled}
        label={t('settings.agents.field.builtinToolDeferred')}
        hint={t('settings.agents.field.builtinToolDeferredHint')}
      />

      <SaveBar onSave={save} isSaving={isSaving} />
    </Section>
  )
}

function QualitySection({
  onSave,
  isSaving,
}: {
  onSave: (patch: Record<string, unknown>) => Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  const cfg = useAgentSettingsStore((s) => s.agentConfig)

  const [thinking, setThinking] = useState<ThinkingLevel>('medium')
  const [evalEnabled, setEvalEnabled] = useState(false)
  const [evalScore, setEvalScore] = useState(0.7)
  const [evalRetries, setEvalRetries] = useState(1)

  useEffect(() => {
    if (!cfg) return
    setThinking((cfg.thinking?.defaultLevel ?? 'medium') as ThinkingLevel)
    setEvalEnabled(cfg.eval?.enabled ?? false)
    setEvalScore(cfg.eval?.minQualityScore ?? 0.7)
    setEvalRetries(cfg.eval?.maxRetries ?? 1)
  }, [cfg])

  async function save() {
    try {
      await onSave({
        thinking: { defaultLevel: thinking },
        eval: {
          enabled: evalEnabled,
          minQualityScore: evalScore,
          maxRetries: evalRetries,
        },
      })
    } catch {

    }
  }

  return (
    <Section
      title={t('settings.agents.section.quality')}
      hint={t('settings.agents.section.qualityHint')}
    >
      <Field
        label={t('settings.agents.field.thinkingLevel')}
        hint={t('settings.agents.field.thinkingLevelHint')}
      >
        <div className="flex flex-wrap gap-1">
          {THINKING_LEVELS.map((lvl) => (
            <button
              key={lvl}
              type="button"
              onClick={() => setThinking(lvl)}
              className={`h-7 px-2.5 rounded-md text-xs border transition-colors ${
                thinking === lvl
                  ? 'border-[var(--color-border-focus)] bg-[var(--color-surface-hover)] text-[var(--color-text-primary)]'
                  : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
              }`}
            >
              {t(`settings.agents.thinking.${lvl}` as never)}
            </button>
          ))}
        </div>
      </Field>

      <CheckboxRow
        checked={evalEnabled}
        onChange={setEvalEnabled}
        label={t('settings.agents.field.evalEnabled')}
        hint={t('settings.agents.field.evalEnabledHint')}
      />
      <Field label={t('settings.agents.field.evalMinScore')}>
        <Input
          type="number"
          step={0.05}
          min={0}
          max={1}
          value={evalScore}
          onChange={(e) => setEvalScore(Number.parseFloat(e.target.value || '0'))}
          disabled={!evalEnabled}
        />
      </Field>
      <NumberField
        label={t('settings.agents.field.evalMaxRetries')}
        value={evalRetries}
        onChange={setEvalRetries}
        min={0}
        disabled={!evalEnabled}
      />

      <SaveBar onSave={save} isSaving={isSaving} />
    </Section>
  )
}

function DirectivesSection({
  onSave,
  isSaving,
}: {
  onSave: (patch: Record<string, unknown>) => Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  const cfg = useAgentSettingsStore((s) => s.agentConfig)

  const [directives, setDirectives] = useState<GlobalDirective[]>([])

  useEffect(() => {
    if (!cfg) return
    setDirectives(cfg.globalDirectives ?? [])
  }, [cfg])

  function updateDirective(idx: number, patch: Partial<GlobalDirective>) {
    setDirectives((arr) => arr.map((d, i) => (i === idx ? { ...d, ...patch } : d)))
  }

  function removeDirective(idx: number) {
    setDirectives((arr) => arr.filter((_, i) => i !== idx))
  }

  function addDirective() {
    setDirectives((arr) => [...arr, { content: '', mode: null }])
  }

  async function save() {
    try {
      await onSave({
        globalDirectives: directives
          .filter((d) => d.content.trim().length > 0)
          .map((d) => ({
            content: d.content.trim(),
            mode: d.mode && d.mode.trim().length > 0 ? d.mode.trim() : null,
          })),
      })
    } catch {

    }
  }

  return (
    <Section
      title={t('settings.agents.section.directives')}
      hint={t('settings.agents.section.directivesHint')}
    >
      <Field
        label={t('settings.agents.field.directives')}
        hint={t('settings.agents.field.directivesHint')}
      >
        <div className="space-y-2">
          {directives.map((d, idx) => (
            <div
              key={idx}
              className="rounded-md border border-[var(--color-border)] p-2 space-y-1"
            >
              <textarea
                className="w-full rounded-md bg-[var(--color-surface)] border border-[var(--color-border)] px-2 py-1 text-xs"
                rows={2}
                placeholder={t('settings.agents.field.directiveContent')}
                value={d.content}
                onChange={(e) => updateDirective(idx, { content: e.target.value })}
              />
              <div className="flex gap-2 items-center">
                <input
                  type="text"
                  placeholder={t('settings.agents.field.directiveModePlaceholder')}
                  value={d.mode ?? ''}
                  onChange={(e) =>
                    updateDirective(idx, { mode: e.target.value.length > 0 ? e.target.value : null })
                  }
                  className="flex-1 rounded-md bg-[var(--color-surface)] border border-[var(--color-border)] px-2 py-1 text-xs"
                />
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  onClick={() => removeDirective(idx)}
                >
                  {t('settings.agents.field.removeDirective')}
                </Button>
              </div>
            </div>
          ))}
          <Button variant="secondary" size="md" type="button" onClick={addDirective}>
            {t('settings.agents.field.addDirective')}
          </Button>
        </div>
      </Field>

      <SaveBar onSave={save} isSaving={isSaving} />
    </Section>
  )
}

function RuntimeSection({
  onSave,
  isSaving,
}: {
  onSave: (patch: Record<string, unknown>) => Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  const cfg = useAgentSettingsStore((s) => s.agentRuntime)

  const [maxIters, setMaxIters] = useState(0)
  const [loopThr, setLoopThr] = useState(0)
  const [parallel, setParallel] = useState(false)
  const [softCap, setSoftCap] = useState(0)
  const [hardCap, setHardCap] = useState(0)
  const [maxSubs, setMaxSubs] = useState(0)
  const [parConc, setParConc] = useState(0)
  const [subTimeout, setSubTimeout] = useState(0)
  const [fastModel, setFastModel] = useState('')
  const [fastTimeout, setFastTimeout] = useState(0)

  const [scEnabled, setScEnabled] = useState(false)
  const [scSamples, setScSamples] = useState(1)
  const [scTemp, setScTemp] = useState(0.7)
  const [scMaxConc, setScMaxConc] = useState(1)
  const [scFinalOnly, setScFinalOnly] = useState(true)

  useEffect(() => {
    if (!cfg) return
    setMaxIters(cfg.maxToolIterations)
    setLoopThr(cfg.loopDetectionThreshold)
    setParallel(cfg.parallelToolsEnabled)
    setSoftCap(cfg.perTurnTokenSoftCap)
    setHardCap(cfg.perTurnTokenHardCap)
    setMaxSubs(cfg.maxSubagents)
    setParConc(cfg.parallelToolMaxConcurrency)
    setSubTimeout(cfg.subagentCallTimeoutSecs)
    setFastModel(cfg.fastApplyModel ?? '')
    setFastTimeout(cfg.fastApplyTimeoutSecs)

    const sc = cfg.selfConsistency
    setScEnabled(sc.enabled)
    setScSamples(sc.samples)
    setScTemp(sc.temperature)
    setScMaxConc(sc.maxConcurrent)
    setScFinalOnly(sc.finalOnly)
  }, [cfg])

  async function save() {
    try {
      await onSave({
        maxToolIterations: maxIters,
        loopDetectionThreshold: loopThr,
        parallelToolsEnabled: parallel,
        perTurnTokenSoftCap: softCap,
        perTurnTokenHardCap: hardCap,
        maxSubagents: maxSubs,
        parallelToolMaxConcurrency: parConc,
        subagentCallTimeoutSecs: subTimeout,
        fastApplyModel: fastModel.trim().length > 0 ? fastModel.trim() : null,
        fastApplyTimeoutSecs: fastTimeout,
        selfConsistency: {
          enabled: scEnabled,
          samples: scSamples,
          temperature: scTemp,
          maxConcurrent: scMaxConc,
          finalOnly: scFinalOnly,
        },
      })
    } catch {

    }
  }

  return (
    <Section
      title={t('settings.agents.section.runtime')}
      hint={t('settings.agents.section.runtimeHint')}
    >
      <NumberField
        label={t('settings.agents.field.runtimeMaxIters')}
        value={maxIters}
        onChange={setMaxIters}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.runtimeLoopThreshold')}
        hint={t('settings.agents.field.runtimeLoopThresholdHint')}
        value={loopThr}
        onChange={setLoopThr}
        min={0}
      />
      <CheckboxRow
        checked={parallel}
        onChange={setParallel}
        label={t('settings.agents.field.runtimeParallel')}
      />
      <NumberField
        label={t('settings.agents.field.runtimeSoftCap')}
        value={softCap}
        onChange={setSoftCap}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.runtimeHardCap')}
        value={hardCap}
        onChange={setHardCap}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.runtimeMaxSubagents')}
        value={maxSubs}
        onChange={setMaxSubs}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.runtimeParallelConcurrency')}
        value={parConc}
        onChange={setParConc}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.runtimeSubagentTimeout')}
        value={subTimeout}
        onChange={setSubTimeout}
        min={0}
      />

      <Field
        label={t('settings.agents.field.fastApplyModel')}
        hint={t('settings.agents.field.fastApplyModelHint')}
      >
        <Input value={fastModel} onChange={(e) => setFastModel(e.target.value)} />
      </Field>
      <NumberField
        label={t('settings.agents.field.fastApplyTimeout')}
        value={fastTimeout}
        onChange={setFastTimeout}
        min={0}
      />

      <div className="border-t border-[var(--color-border)] my-2" />

      <CheckboxRow
        checked={scEnabled}
        onChange={setScEnabled}
        label={t('settings.agents.field.selfConsistencyEnabled')}
        hint={t('settings.agents.field.selfConsistencyEnabledHint')}
      />
      <NumberField
        label={t('settings.agents.field.selfConsistencySamples')}
        value={scSamples}
        onChange={setScSamples}
        min={1}
        disabled={!scEnabled}
      />
      <Field label={t('settings.agents.field.selfConsistencyTemperature')}>
        <Input
          type="number"
          step={0.05}
          min={0}
          max={2}
          value={scTemp}
          onChange={(e) => setScTemp(Number.parseFloat(e.target.value || '0'))}
          disabled={!scEnabled}
        />
      </Field>
      <NumberField
        label={t('settings.agents.field.selfConsistencyMaxConcurrent')}
        value={scMaxConc}
        onChange={setScMaxConc}
        min={1}
        disabled={!scEnabled}
      />
      <CheckboxRow
        checked={scFinalOnly}
        onChange={setScFinalOnly}
        label={t('settings.agents.field.selfConsistencyFinalOnly')}
        disabled={!scEnabled}
      />

      <SaveBar onSave={save} isSaving={isSaving} />
    </Section>
  )
}

function ContextToolsSection({
  onSaveSearch,
  onSaveFetch,
  isSaving,
}: {
  onSaveSearch: (patch: Record<string, unknown>) => Promise<void>
  onSaveFetch: (patch: Record<string, unknown>) => Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  const ws = useAgentSettingsStore((s) => s.webSearch)
  const wf = useAgentSettingsStore((s) => s.webFetch)

  const [wsEnabled, setWsEnabled] = useState(false)
  const [wsProvider, setWsProvider] = useState('duckduckgo')
  const [wsMax, setWsMax] = useState(5)
  const [wsTimeout, setWsTimeout] = useState(15)
  const [wsBrave, setWsBrave] = useState('')
  const [wsSearxng, setWsSearxng] = useState('')
  const [wsTavily, setWsTavily] = useState('')
  const [wsExa, setWsExa] = useState('')

  const [wfEnabled, setWfEnabled] = useState(false)
  const [wfAllowed, setWfAllowed] = useState('')
  const [wfBlocked, setWfBlocked] = useState('')
  const [wfPrivate, setWfPrivate] = useState('')
  const [wfMaxSize, setWfMaxSize] = useState(0)
  const [wfTimeout, setWfTimeout] = useState(0)

  useEffect(() => {
    if (!ws) return
    setWsEnabled(ws.enabled)
    setWsProvider(ws.provider || 'duckduckgo')
    setWsMax(ws.maxResults)
    setWsTimeout(ws.timeoutSecs)
    setWsBrave(ws.braveApiKey ?? '')
    setWsSearxng(ws.searxngInstanceUrl ?? '')
    setWsTavily(ws.tavilyApiKey ?? '')
    setWsExa(ws.exaApiKey ?? '')
  }, [ws])

  useEffect(() => {
    if (!wf) return
    setWfEnabled(wf.enabled)
    setWfAllowed((wf.allowedDomains ?? []).join(', '))
    setWfBlocked((wf.blockedDomains ?? []).join(', '))
    setWfPrivate((wf.allowedPrivateHosts ?? []).join(', '))
    setWfMaxSize(wf.maxResponseSize)
    setWfTimeout(wf.timeoutSecs)
  }, [wf])

  async function saveSearch() {
    try {
      await onSaveSearch({
        enabled: wsEnabled,
        provider: wsProvider,
        maxResults: wsMax,
        timeoutSecs: wsTimeout,
        braveApiKey: wsBrave.trim().length > 0 ? wsBrave.trim() : null,
        searxngInstanceUrl: wsSearxng.trim().length > 0 ? wsSearxng.trim() : null,
        tavilyApiKey: wsTavily.trim().length > 0 ? wsTavily.trim() : null,
        exaApiKey: wsExa.trim().length > 0 ? wsExa.trim() : null,
      })
    } catch {

    }
  }

  async function saveFetch() {
    try {
      await onSaveFetch({
        enabled: wfEnabled,
        allowedDomains: splitList(wfAllowed),
        blockedDomains: splitList(wfBlocked),
        allowedPrivateHosts: splitList(wfPrivate),
        maxResponseSize: wfMaxSize,
        timeoutSecs: wfTimeout,
      })
    } catch {

    }
  }

  const providerOptions = useMemo(() => SEARCH_PROVIDERS, [])

  return (
    <Section
      title={t('settings.agents.section.contextTools')}
      hint={t('settings.agents.section.contextToolsHint')}
    >
      <CheckboxRow
        checked={wsEnabled}
        onChange={setWsEnabled}
        label={t('settings.agents.field.webSearchEnabled')}
      />
      <Field label={t('settings.agents.field.webSearchProvider')}>
        <Select value={wsProvider} onChange={setWsProvider} options={providerOptions} />
      </Field>
      <NumberField
        label={t('settings.agents.field.webSearchMaxResults')}
        value={wsMax}
        onChange={setWsMax}
        min={1}
        max={10}
      />
      <NumberField
        label={t('settings.agents.field.webSearchTimeout')}
        value={wsTimeout}
        onChange={setWsTimeout}
        min={0}
      />
      <Field
        label={t('settings.agents.field.webSearchBraveKey')}
        hint={t('settings.agents.field.webSearchSecretsHint')}
      >
        <Input value={wsBrave} onChange={(e) => setWsBrave(e.target.value)} type="password" />
      </Field>
      <Field label={t('settings.agents.field.webSearchSearxng')}>
        <Input value={wsSearxng} onChange={(e) => setWsSearxng(e.target.value)} />
      </Field>
      <Field label={t('settings.agents.field.webSearchTavilyKey')}>
        <Input value={wsTavily} onChange={(e) => setWsTavily(e.target.value)} type="password" />
      </Field>
      <Field label={t('settings.agents.field.webSearchExaKey')}>
        <Input value={wsExa} onChange={(e) => setWsExa(e.target.value)} type="password" />
      </Field>
      <SaveBar onSave={saveSearch} isSaving={isSaving} />

      <div className="border-t border-[var(--color-border)] my-2" />

      <CheckboxRow
        checked={wfEnabled}
        onChange={setWfEnabled}
        label={t('settings.agents.field.webFetchEnabled')}
      />
      <Field label={t('settings.agents.field.webFetchAllowed')}>
        <Input value={wfAllowed} onChange={(e) => setWfAllowed(e.target.value)} />
      </Field>
      <Field label={t('settings.agents.field.webFetchBlocked')}>
        <Input value={wfBlocked} onChange={(e) => setWfBlocked(e.target.value)} />
      </Field>
      <Field label={t('settings.agents.field.webFetchPrivate')}>
        <Input value={wfPrivate} onChange={(e) => setWfPrivate(e.target.value)} />
      </Field>
      <NumberField
        label={t('settings.agents.field.webFetchMaxSize')}
        value={wfMaxSize}
        onChange={setWfMaxSize}
        min={0}
      />
      <NumberField
        label={t('settings.agents.field.webFetchTimeout')}
        value={wfTimeout}
        onChange={setWfTimeout}
        min={0}
      />

      <SaveBar onSave={saveFetch} isSaving={isSaving} />
    </Section>
  )
}

function Section({
  title,
  hint,
  children,
}: {
  title: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-4 space-y-3">
      <div>
        <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">{title}</h3>
        {hint && (
          <p className="text-[11px] text-[var(--color-text-tertiary)] mt-0.5">{hint}</p>
        )}
      </div>
      {children}
    </section>
  )
}

function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <label className="block space-y-1">
      <span className="text-xs font-medium text-[var(--color-text-secondary)]">{label}</span>
      {children}
      {hint && (
        <span className="block text-[11px] text-[var(--color-text-tertiary)]">{hint}</span>
      )}
    </label>
  )
}

function CheckboxRow({
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  checked: boolean
  onChange: (next: boolean) => void
  label: string
  hint?: string
  disabled?: boolean
}) {
  return (
    <label className={`flex items-start gap-2 text-xs ${disabled ? 'opacity-60' : ''}`}>
      <input
        type="checkbox"
        className="mt-[2px]"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        disabled={disabled}
      />
      <span>
        <span className="text-[var(--color-text-primary)] font-medium">{label}</span>
        {hint && (
          <span className="block text-[11px] text-[var(--color-text-tertiary)]">{hint}</span>
        )}
      </span>
    </label>
  )
}

function NumberField({
  label,
  hint,
  value,
  onChange,
  min,
  max,
  disabled,
}: {
  label: string
  hint?: string
  value: number
  onChange: (next: number) => void
  min?: number
  max?: number
  disabled?: boolean
}) {
  return (
    <Field label={label} hint={hint}>
      <Input
        type="number"
        min={min}
        max={max}
        value={Number.isFinite(value) ? value : 0}
        onChange={(e) => onChange(Number.parseInt(e.target.value || '0', 10))}
        disabled={disabled}
      />
    </Field>
  )
}

function Select({
  value,
  onChange,
  options,
  disabled,
}: {
  value: string
  onChange: (next: string) => void
  options: string[]
  disabled?: boolean
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
      className="h-10 px-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] focus:shadow-[var(--shadow-focus-ring)] disabled:opacity-60"
    >
      {options.map((opt) => (
        <option key={opt} value={opt}>
          {opt}
        </option>
      ))}
    </select>
  )
}

function SaveBar({
  onSave,
  isSaving,
}: {
  onSave: () => void | Promise<void>
  isSaving: boolean
}) {
  const t = useTranslation()
  return (
    <div className="flex items-center gap-2 pt-1">
      <Button onClick={() => void onSave()} disabled={isSaving} size="md">
        {isSaving ? t('common.saving') : t('common.save')}
      </Button>
    </div>
  )
}
