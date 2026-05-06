import { useState, useEffect, useMemo } from 'react'
import { useSettingsStore } from '../stores/settingsStore'
import { useProviderStore } from '../stores/providerStore'
import { useTranslation } from '../i18n'
import { Modal } from '../components/shared/Modal'
import { ConfirmDialog } from '../components/shared/ConfirmDialog'
import { Input } from '../components/shared/Input'
import { Button } from '../components/shared/Button'
import type { EffortLevel, ThemeMode } from '../types/settings'
import type { Locale } from '../i18n'
import type { SavedProvider, UpdateProviderInput, ProviderTestResult, ApiFormat } from '../types/provider'
import type { ProviderPreset } from '../types/providerPreset'
import { AdapterSettings } from './AdapterSettings'
import { ToolsAndMcpsSettings } from './ToolsAndMcpsSettings'
import { HooksSettings } from './HooksSettings'
import { UsageSettings } from './UsageSettings'
import { RulesSkillsSubagentsSettings } from './RulesSkillsSubagentsSettings'
import { AgentsSettings } from './AgentsSettings'
import { LspSettings } from './LspSettings'
import { KeyboardShortcutsSettings } from './KeyboardShortcutsSettings'
import { useUIStore, type SettingsTab } from '../stores/uiStore'

export function Settings() {
  const [activeTab, setActiveTab] = useState<SettingsTab>('general')
  const pendingSettingsTab = useUIStore((s) => s.pendingSettingsTab)
  const t = useTranslation()

  useEffect(() => {
    if (!pendingSettingsTab) return
    setActiveTab(pendingSettingsTab)
    useUIStore.getState().setPendingSettingsTab(null)
  }, [pendingSettingsTab])

  const closeSettingsOverlay = useUIStore((s) => s.closeSettingsOverlay)

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-[var(--color-surface)]">
      <div className="flex-1 flex overflow-hidden">
        {}
        <div className="w-[160px] border-r border-[var(--color-border)] py-3 flex-shrink-0 flex flex-col">
          <div className="flex-1 overflow-y-auto">
            <TabButton icon="tune" label={t('settings.tab.general')} active={activeTab === 'general'} onClick={() => setActiveTab('general')} />
            <TabButton icon="chat" label={t('settings.tab.adapters')} active={activeTab === 'adapters'} onClick={() => setActiveTab('adapters')} />
            <TabButton icon="bar_chart" label={t('settings.tab.usage')} active={activeTab === 'usage'} onClick={() => setActiveTab('usage')} />
            <TabButton icon="dns" label={t('settings.tab.providers')} active={activeTab === 'providers'} onClick={() => setActiveTab('providers')} />
            <TabButton icon="smart_toy" label={t('settings.tab.agents')} active={activeTab === 'agents'} onClick={() => setActiveTab('agents')} />
            <TabButton icon="psychology" label={t('settings.tab.codingMode')} active={activeTab === 'codingMode'} onClick={() => setActiveTab('codingMode')} />
            <TabButton icon="policy" label={t('settings.tab.skills')} active={activeTab === 'skills'} onClick={() => setActiveTab('skills')} />
            <TabButton icon="build" label={t('settings.tab.mcp')} active={activeTab === 'mcp'} onClick={() => setActiveTab('mcp')} />
            <TabButton icon="code" label={t('settings.tab.lsp')} active={activeTab === 'lsp'} onClick={() => setActiveTab('lsp')} />
            <TabButton icon="keyboard" label={t('settings.tab.keyboard')} active={activeTab === 'keyboard'} onClick={() => setActiveTab('keyboard')} />
            <TabButton icon="webhook" label={t('settings.tab.hooks')} active={activeTab === 'hooks'} onClick={() => setActiveTab('hooks')} />
          </div>
          <div className="flex-shrink-0 border-t border-[var(--color-border)] pt-2 mt-2">
            <TabButton
              icon="arrow_back"
              label={t('settings.backToHome')}
              active={false}
              onClick={closeSettingsOverlay}
            />
          </div>
        </div>

        {}
        <div className="flex-1 overflow-y-auto px-6 py-4">
          {activeTab === 'providers' && <ProviderSettings />}
          {activeTab === 'agents' && <AgentsSettings />}
          {activeTab === 'codingMode' && <CodingModeSettings />}
          {activeTab === 'general' && <GeneralSettings />}
          {activeTab === 'adapters' && <AdapterSettings />}
          {activeTab === 'mcp' && <ToolsAndMcpsSettings />}
          {activeTab === 'lsp' && <LspSettings />}
          {activeTab === 'keyboard' && <KeyboardShortcutsSettings />}
          {activeTab === 'skills' && <RulesSkillsSubagentsSettings />}
          {activeTab === 'hooks' && <HooksSettings />}
          {activeTab === 'usage' && <UsageSettings />}
        </div>
      </div>
    </div>
  )
}

function TabButton({ icon, label, active, onClick }: { icon: string; label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={`w-full flex items-center gap-2 px-3 py-2 text-xs text-left transition-colors ${
        active
          ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)] font-medium'
          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
      }`}
    >
      <span className="material-symbols-outlined text-[14px]">{icon}</span>
      {label}
    </button>
  )
}

function ProviderSettings() {
  const providers = useProviderStore((s) => s.providers)
  const activeId = useProviderStore((s) => s.activeId)
  const presets = useProviderStore((s) => s.presets)
  const isLoading = useProviderStore((s) => s.isLoading)
  const isPresetsLoading = useProviderStore((s) => s.isPresetsLoading)
  const fetchProviders = useProviderStore((s) => s.fetchProviders)
  const fetchPresets = useProviderStore((s) => s.fetchPresets)
  const deleteProvider = useProviderStore((s) => s.deleteProvider)
  const activateProvider = useProviderStore((s) => s.activateProvider)
  const testProvider = useProviderStore((s) => s.testProvider)
  const updateProvider = useProviderStore((s) => s.updateProvider)
  const fetchSettings = useSettingsStore((s) => s.fetchAll)
  const t = useTranslation()
  const [editingProvider, setEditingProvider] = useState<SavedProvider | null>(null)
  const [showCreateModal, setShowCreateModal] = useState(false)
  const [pendingDeleteProvider, setPendingDeleteProvider] = useState<SavedProvider | null>(null)
  const [isDeletingProvider, setIsDeletingProvider] = useState(false)
  const [testResults, setTestResults] = useState<Record<string, { loading: boolean; result?: ProviderTestResult }>>({})
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})

  useEffect(() => {
    void fetchProviders()
    void fetchPresets()
  }, [fetchPresets, fetchProviders])

  const presetMap = useMemo(
    () => new Map(presets.map((preset) => [preset.id, preset])),
    [presets],
  )

  const handleDelete = async (provider: SavedProvider) => {
    if (activeId === provider.id) return
    setPendingDeleteProvider(provider)
  }

  const confirmDelete = async () => {
    if (!pendingDeleteProvider) return
    setIsDeletingProvider(true)
    try {
      await deleteProvider(pendingDeleteProvider.id)
      setPendingDeleteProvider(null)
    } catch (error) {
      console.error(error)
    } finally {
      setIsDeletingProvider(false)
    }
  }

  const handleTest = async (provider: SavedProvider) => {
    setTestResults((r) => ({ ...r, [provider.id]: { loading: true } }))
    try {
      const result = await testProvider(provider.id)
      setTestResults((r) => ({ ...r, [provider.id]: { loading: false, result } }))
    } catch {
      setTestResults((r) => ({ ...r, [provider.id]: { loading: false, result: { connectivity: { success: false, latencyMs: 0, error: t('settings.providers.requestFailed') } } } }))
    }
  }

  const handleActivate = async (id: string) => {
    await activateProvider(id)
    await fetchSettings()
  }

  const toggleExpanded = (id: string) => {
    setExpanded((prev) => ({ ...prev, [id]: !prev[id] }))
  }

  const removeModelFromProvider = async (provider: SavedProvider, modelId: string) => {
    const next = provider.models.filter((m) => m !== modelId)
    await updateProvider(provider.id, { models: next })
  }

  return (
    <div className="max-w-2xl">
      <div className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-base font-semibold text-[var(--color-text-primary)]">{t('settings.providers.title')}</h2>
          <p className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{t('settings.providers.description')}</p>
        </div>
        <Button size="md" onClick={() => setShowCreateModal(true)} disabled={isPresetsLoading || presets.length === 0}>
          <span className="material-symbols-outlined text-[16px]">add</span>
          {t('settings.providers.addProvider')}
        </Button>
      </div>

      {}
      {isLoading && providers.length === 0 ? (
        <div className="flex justify-center py-8">
          <div className="animate-spin w-5 h-5 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
        </div>
      ) : providers.length === 0 ? (
        <div className="text-xs text-[var(--color-text-tertiary)] py-6 text-center border border-dashed border-[var(--color-border)] rounded-xl">
          {t('settings.providers.empty')}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {providers.map((provider) => {
            const isActive = activeId === provider.id
            const test = testResults[provider.id]
            const preset = presetMap.get(provider.presetId)
            const isExpanded = expanded[provider.id] ?? isActive
            return (
              <div
                key={provider.id}
                className={`relative flex flex-col rounded-xl border transition-all group ${
                  isActive
                    ? 'border-[var(--color-brand)] bg-[var(--color-surface-container)] shadow-[var(--shadow-focus-ring)]'
                    : 'border-[var(--color-border)] hover:border-[var(--color-border-focus)]'
                }`}
              >
                <div className="flex items-center gap-2 px-3 py-2.5">
                  <button
                    onClick={() => toggleExpanded(provider.id)}
                    className="flex-shrink-0 w-6 h-6 flex items-center justify-center rounded hover:bg-[var(--color-surface-hover)]"
                    title={isExpanded ? t('common.collapse') : t('common.expand')}
                  >
                    <span className="material-symbols-outlined text-[18px] text-[var(--color-text-secondary)]">
                      {isExpanded ? 'expand_more' : 'chevron_right'}
                    </span>
                  </button>
                  <span className={`w-2.5 h-2.5 rounded-full flex-shrink-0 ${isActive ? 'bg-[var(--color-success)]' : 'bg-[var(--color-text-tertiary)]'}`} />
                  <div className="flex-1 min-w-0 cursor-pointer" onClick={() => toggleExpanded(provider.id)}>
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-semibold text-[var(--color-text-primary)] truncate">{provider.name}</span>
                      {preset && preset.id !== 'custom' && (
                        <span className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] leading-none">{preset.name}</span>
                      )}
                      <span className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] leading-none">
                        {t('settings.providers.modelCount', { count: String(provider.models.length) })}
                      </span>
                      {provider.apiFormat && provider.apiFormat !== 'anthropic' && (
                        <span className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-warning)] leading-none">
                          {provider.apiFormat === 'openai_chat' ? 'OpenAI Chat' : 'OpenAI Responses'}
                        </span>
                      )}
                      {isActive && (
                        <span className="px-1.5 py-0.5 text-[10px] font-bold rounded border border-[var(--color-brand)]/18 bg-[var(--color-brand)]/14 text-[var(--color-brand)] leading-none">{t('settings.providers.default')}</span>
                      )}
                    </div>
                    <div className="text-xs text-[var(--color-text-tertiary)] truncate mt-0.5">
                      {provider.baseUrl || t('settings.providers.noBaseUrl')}
                    </div>
                    {test && !test.loading && test.result && (
                      <div className="text-xs mt-1 flex flex-col gap-0.5">
                        <span className={test.result.connectivity.success ? 'text-[var(--color-success)]' : 'text-[var(--color-error)]'}>
                          {test.result.connectivity.success
                            ? t('settings.providers.connectivityOk', { latency: String(test.result.connectivity.latencyMs) })
                            : t('settings.providers.connectivityFailed', { error: test.result.connectivity.error || '' })}
                        </span>
                        {test.result.proxy && (
                          <span className={test.result.proxy.success ? 'text-[var(--color-success)]' : 'text-[var(--color-error)]'}>
                            {test.result.proxy.success
                              ? t('settings.providers.proxyOk', { latency: String(test.result.proxy.latencyMs) })
                              : t('settings.providers.proxyFailed', { error: test.result.proxy.error || '' })}
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                  <div className="flex items-center gap-1 flex-shrink-0">
                    {!isActive && (
                      <Button variant="ghost" size="sm" onClick={() => handleActivate(provider.id)}>{t('settings.providers.setDefault')}</Button>
                    )}
                    <Button variant="ghost" size="sm" onClick={() => handleTest(provider)} loading={test?.loading}>{t('settings.providers.test')}</Button>
                    <Button variant="ghost" size="sm" onClick={() => setEditingProvider(provider)}>{t('settings.providers.edit')}</Button>
                    {!isActive && (
                      <Button variant="ghost" size="sm" onClick={() => handleDelete(provider)} className="text-[var(--color-error)] hover:text-[var(--color-error)]">{t('common.delete')}</Button>
                    )}
                  </div>
                </div>

                {isExpanded && (
                  <div className="px-4 pb-3 pt-1 border-t border-[var(--color-border-separator)]">
                    <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)] mb-2">
                      {t('settings.providers.modelsHeader')}
                    </div>
                    {provider.models.length === 0 ? (
                      <div className="text-xs text-[var(--color-text-tertiary)] italic py-2">
                        {t('settings.providers.noModels')}
                      </div>
                    ) : (
                      <div className="flex flex-col gap-1">
                        {provider.models.map((modelId) => (
                          <div
                            key={modelId}
                            className="flex items-center justify-between gap-2 px-3 py-1.5 rounded-md bg-[var(--color-surface-container-low)] border border-[var(--color-border)]"
                          >
                            <span className="text-xs font-mono text-[var(--color-text-primary)] truncate">{modelId}</span>
                            <button
                              onClick={() => removeModelFromProvider(provider, modelId)}
                              className="text-[var(--color-text-tertiary)] hover:text-[var(--color-error)] flex-shrink-0"
                              title={t('common.delete')}
                            >
                              <span className="material-symbols-outlined text-[16px]">close</span>
                            </button>
                          </div>
                        ))}
                      </div>
                    )}
                    <div className="mt-2 flex justify-end">
                      <Button variant="ghost" size="sm" onClick={() => setEditingProvider(provider)}>
                        <span className="material-symbols-outlined text-[14px]">edit</span>
                        {t('settings.providers.editModels')}
                      </Button>
                    </div>
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}

      {}
      {showCreateModal && (
        <ProviderFormModal open={true} onClose={() => setShowCreateModal(false)} mode="create" presets={presets} />
      )}

      {}
      {editingProvider && (
        <ProviderFormModal key={editingProvider.id} open={true} onClose={() => setEditingProvider(null)} mode="edit" provider={editingProvider} presets={presets} />
      )}

      <ConfirmDialog
        open={pendingDeleteProvider !== null}
        onClose={() => {
          if (isDeletingProvider) return
          setPendingDeleteProvider(null)
        }}
        onConfirm={confirmDelete}
        title={t('common.delete')}
        body={pendingDeleteProvider ? t('settings.providers.confirmDelete', { name: pendingDeleteProvider.name }) : ''}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        confirmVariant="danger"
        loading={isDeletingProvider}
      />
    </div>
  )
}

type ProviderFormProps = {
  open: boolean
  onClose: () => void
  mode: 'create' | 'edit'
  provider?: SavedProvider
  presets: ProviderPreset[]
}

function requirePreset(preset: ProviderPreset | undefined): ProviderPreset {
  if (!preset) {
    throw new Error('Provider presets are not configured')
  }
  return preset
}

function buildFallbackPreset(provider?: SavedProvider): ProviderPreset {
  return {
    id: provider?.presetId ?? 'custom',
    name: provider?.name ?? 'Custom',
    baseUrl: provider?.baseUrl ?? '',
    apiFormat: provider?.apiFormat ?? 'openai_chat',
    defaultModels: provider?.models ?? [],
    needsApiKey: true,
    websiteUrl: '',
  }
}

function ProviderFormModal({ open, onClose, mode, provider, presets }: ProviderFormProps) {
  const createProvider = useProviderStore((s) => s.createProvider)
  const updateProvider = useProviderStore((s) => s.updateProvider)
  const testConfig = useProviderStore((s) => s.testConfig)
  const fetchSettings = useSettingsStore((s) => s.fetchAll)
  const t = useTranslation()

  const availablePresets = presets.filter((p) => p.id !== 'official')
  const fallbackPreset = provider
    ? buildFallbackPreset(provider)
    : requirePreset(availablePresets[availablePresets.length - 1])
  const initialPreset = requirePreset(
    provider
      ? availablePresets.find((p) => p.id === provider.presetId) ?? fallbackPreset
      : availablePresets[0] ?? fallbackPreset,
  )

  const [selectedPreset, setSelectedPreset] = useState<ProviderPreset>(initialPreset)
  const [name, setName] = useState(provider?.name ?? initialPreset.name)
  const [baseUrl, setBaseUrl] = useState(provider?.baseUrl ?? initialPreset.baseUrl)
  const [apiFormat, setApiFormat] = useState<ApiFormat>(provider?.apiFormat ?? initialPreset.apiFormat ?? 'openai_chat')
  const [apiKey, setApiKey] = useState('')
  const [notes, setNotes] = useState(provider?.notes ?? '')
  const [models, setModels] = useState<string[]>(
    provider?.models ?? [...initialPreset.defaultModels],
  )

  const [modelContextWindows, setModelContextWindows] = useState<Record<string, string>>(() => {
    const initial: Record<string, string> = {}
    if (provider?.modelContextWindows) {
      for (const [key, value] of Object.entries(provider.modelContextWindows)) {
        if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
          initial[key] = String(value)
        }
      }
    }
    return initial
  })
  const [newModelDraft, setNewModelDraft] = useState('')
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null)
  const [isTesting, setIsTesting] = useState(false)

  const handlePresetChange = (preset: ProviderPreset) => {
    setSelectedPreset(preset)
    setName(preset.name)
    setBaseUrl(preset.baseUrl)
    setApiFormat(preset.apiFormat ?? 'openai_chat')
    setModels([...preset.defaultModels])

    setModelContextWindows({})
    setTestResult(null)
  }

  const isCustom = selectedPreset.id === 'custom'
  const trimmedModels = useMemo(
    () =>
      Array.from(
        new Set(
          models
            .map((m) => m.trim())
            .filter((m) => m.length > 0),
        ),
      ),
    [models],
  )
  const primaryModel = trimmedModels[0] ?? ''
  const canSubmit =
    name.trim().length > 0 &&
    baseUrl.trim().length > 0 &&
    (mode === 'edit' || apiKey.trim().length > 0) &&
    trimmedModels.length > 0

  const addModel = (raw?: string) => {
    const candidate = (raw ?? newModelDraft).trim()
    if (!candidate) return
    setModels((prev) => (prev.includes(candidate) ? prev : [...prev, candidate]))
    setNewModelDraft('')
  }

  const removeModel = (modelId: string) => {
    setModels((prev) => prev.filter((m) => m !== modelId))
    setModelContextWindows((prev) => {
      if (!(modelId in prev)) return prev
      const { [modelId]: _removed, ...rest } = prev
      return rest
    })
  }

  const updateModel = (index: number, value: string) => {
    setModels((prev) => {
      const previous = prev[index]
      const next = prev.map((m, i) => (i === index ? value : m))
      if (previous && previous !== value) {
        setModelContextWindows((existing) => {
          const carried = existing[previous]
          if (carried === undefined) return existing
          const { [previous]: _removed, ...rest } = existing
          const trimmedNext = value.trim()
          if (!trimmedNext) return rest
          const merged: Record<string, string> = { ...rest }
          merged[trimmedNext] = carried
          return merged
        })
      }
      return next
    })
  }

  const updateModelContextWindow = (modelId: string, raw: string) => {
    setModelContextWindows((prev) => {
      const trimmed = raw.trim()
      if (!trimmed) {
        if (!(modelId in prev)) return prev
        const { [modelId]: _removed, ...rest } = prev
        return rest
      }

      const sanitized = trimmed.replace(/[^0-9]/g, '')
      if (!sanitized) {
        if (!(modelId in prev)) return prev
        const { [modelId]: _removed, ...rest } = prev
        return rest
      }
      return { ...prev, [modelId]: sanitized }
    })
  }

  const buildContextWindowsPayload = (): Record<string, number> => {
    const payload: Record<string, number> = {}
    const seen = new Set(trimmedModels)
    for (const [model, raw] of Object.entries(modelContextWindows)) {
      if (!seen.has(model)) continue
      const parsed = Number.parseInt(raw, 10)
      if (Number.isFinite(parsed) && parsed > 0) {
        payload[model] = parsed
      }
    }
    return payload
  }

  const handleSubmit = async () => {
    if (!canSubmit) return
    setIsSubmitting(true)
    try {
      const overridesPayload = buildContextWindowsPayload()
      if (mode === 'create') {
        await createProvider({
          presetId: selectedPreset.id,
          name: name.trim(),
          apiKey: apiKey.trim(),
          baseUrl: baseUrl.trim(),
          apiFormat,
          models: trimmedModels,
          modelContextWindows: overridesPayload,
          notes: notes.trim() || undefined,
        })
      } else if (provider) {
        const input: UpdateProviderInput = {
          name: name.trim(),
          baseUrl: baseUrl.trim(),
          apiFormat,
          models: trimmedModels,
          modelContextWindows: overridesPayload,
          notes: notes.trim() || undefined,
        }
        if (apiKey.trim()) input.apiKey = apiKey.trim()
        await updateProvider(provider.id, input)
      }
      await fetchSettings()
      onClose()
    } catch (err) {
      console.error('Failed to save provider:', err)
    } finally {
      setIsSubmitting(false)
    }
  }

  const handleTest = async () => {
    if (!baseUrl.trim() || !primaryModel) return
    setIsTesting(true)
    setTestResult(null)
    try {
      let result: ProviderTestResult
      if (mode === 'edit' && provider && !apiKey.trim()) {
        result = await useProviderStore.getState().testProvider(provider.id, {
          baseUrl: baseUrl.trim(),
          modelId: primaryModel,
          apiFormat,
        })
      } else {
        if (!apiKey.trim()) return
        result = await testConfig({ baseUrl: baseUrl.trim(), apiKey: apiKey.trim(), modelId: primaryModel, apiFormat })
      }
      setTestResult(result)
    } catch {
      setTestResult({ connectivity: { success: false, latencyMs: 0, error: t('settings.providers.requestFailed') } })
    } finally {
      setIsTesting(false)
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={mode === 'create' ? t('settings.providers.addTitle') : t('settings.providers.editTitle')}
      width={640}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>{t('common.cancel')}</Button>
          <Button onClick={handleSubmit} disabled={!canSubmit} loading={isSubmitting}>
            {mode === 'create' ? t('common.add') : t('common.save')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {}
        {mode === 'create' && (
          <div>
            <label className="text-xs font-medium text-[var(--color-text-primary)] mb-2 block">{t('settings.providers.preset')}</label>
            <div className="flex flex-wrap gap-2">
              {availablePresets.map((preset) => (
                <button
                  key={preset.id}
                  onClick={() => handlePresetChange(preset)}
                  className={`px-3 py-1.5 text-xs font-medium rounded-full border transition-all ${
                    selectedPreset.id === preset.id
                      ? 'border-[var(--color-brand)] bg-[var(--color-surface-container-high)] text-[var(--color-brand)] shadow-[var(--shadow-focus-ring)]'
                      : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:border-[var(--color-border-focus)] hover:bg-[var(--color-surface-hover)]'
                  }`}
                >
                  {preset.name}
                </button>
              ))}
            </div>
          </div>
        )}

        <Input label={t('settings.providers.name')} required value={name} onChange={(e) => setName(e.target.value)} placeholder={t('settings.providers.namePlaceholder')} />

        <Input label={t('settings.providers.notes')} value={notes} onChange={(e) => setNotes(e.target.value)} placeholder={t('settings.providers.notesPlaceholder')} />

        {}
        {isCustom || mode === 'edit' ? (
          <Input label={t('settings.providers.baseUrl')} required value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder={t('settings.providers.baseUrlPlaceholder')} />
        ) : (
          <div>
            <label className="text-xs font-medium text-[var(--color-text-primary)] mb-1 block">{t('settings.providers.baseUrl')}</label>
            <div className="text-xs text-[var(--color-text-tertiary)] px-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)]">
              {baseUrl}
            </div>
          </div>
        )}

        {}
        <div>
          <label className="text-xs font-medium text-[var(--color-text-primary)] mb-1 block">{t('settings.providers.apiFormat')}</label>
          <select
            value={apiFormat}
            onChange={(e) => setApiFormat(e.target.value as ApiFormat)}
            className="w-full text-xs px-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
          >
            <option value="openai_chat">{t('settings.providers.apiFormatOpenaiChat')}</option>
            <option value="openai_responses">{t('settings.providers.apiFormatOpenaiResponses')}</option>
            <option value="anthropic">{t('settings.providers.apiFormatAnthropic')}</option>
          </select>
        </div>

        <Input
          label={mode === 'edit' ? t('settings.providers.apiKeyKeep') : t('settings.providers.apiKey')}
          required={mode === 'create'}
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          placeholder={mode === 'edit' ? '****' : 'sk-...'}
        />

        {}
        <div>
          <label className="text-xs font-medium text-[var(--color-text-primary)] mb-2 block">
            {t('settings.providers.modelsHeader')}
            <span className="ml-2 text-[11px] font-normal text-[var(--color-text-tertiary)]">{t('settings.providers.modelsHelp')}</span>
          </label>
          <div className="flex flex-col gap-1.5">
            {models.length === 0 && (
              <div className="text-xs italic text-[var(--color-text-tertiary)] px-3 py-2 rounded-md border border-dashed border-[var(--color-border)]">
                {t('settings.providers.noModels')}
              </div>
            )}
            {models.map((m, idx) => {
              const trimmedModelId = m.trim()
              const contextWindowDraft = trimmedModelId
                ? modelContextWindows[trimmedModelId] ?? ''
                : ''
              return (
                <div key={`${idx}-${m}`} className="flex items-center gap-2">
                  <input
                    value={m}
                    onChange={(e) => updateModel(idx, e.target.value)}
                    placeholder="model-id"
                    className="flex-1 text-xs font-mono px-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
                  />
                  <input
                    value={contextWindowDraft}
                    onChange={(e) =>
                      trimmedModelId &&
                      updateModelContextWindow(trimmedModelId, e.target.value)
                    }
                    inputMode="numeric"
                    pattern="[0-9]*"
                    placeholder={t('settings.providers.contextWindowPlaceholder')}
                    title={t('settings.providers.contextWindowTooltip')}
                    disabled={!trimmedModelId}
                    className="w-28 text-xs font-mono px-2 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50"
                  />
                  {idx === 0 && (
                    <span className="px-1.5 py-0.5 text-[10px] font-bold rounded border border-[var(--color-brand)]/18 bg-[var(--color-brand)]/14 text-[var(--color-brand)] leading-none">
                      {t('settings.providers.primaryTag')}
                    </span>
                  )}
                  <button
                    onClick={() => removeModel(m)}
                    className="text-[var(--color-text-tertiary)] hover:text-[var(--color-error)] flex-shrink-0"
                    title={t('common.delete')}
                  >
                    <span className="material-symbols-outlined text-[18px]">close</span>
                  </button>
                </div>
              )
            })}
            <div className="flex items-center gap-2 mt-1">
              <input
                value={newModelDraft}
                onChange={(e) => setNewModelDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    addModel()
                  }
                }}
                placeholder={t('settings.providers.addModelPlaceholder')}
                className="flex-1 text-xs font-mono px-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-dashed border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
              />
              <Button variant="secondary" size="md" onClick={() => addModel()} disabled={!newModelDraft.trim()}>
                <span className="material-symbols-outlined text-[16px]">add</span>
                {t('settings.providers.addModel')}
              </Button>
            </div>
          </div>
        </div>

        {}
        <div className="flex items-center gap-3">
          <Button variant="secondary" size="md" onClick={handleTest} loading={isTesting} disabled={!baseUrl.trim() || !primaryModel}>
            {t('settings.providers.testConnection')}
          </Button>
          {testResult && (
            <div className="flex flex-col gap-0.5">
              <span className={`text-xs ${testResult.connectivity.success ? 'text-[var(--color-success)]' : 'text-[var(--color-error)]'}`}>
                {testResult.connectivity.success
                  ? t('settings.providers.connectivityOk', { latency: String(testResult.connectivity.latencyMs) })
                  : t('settings.providers.connectivityFailed', { error: testResult.connectivity.error || '' })}
              </span>
              {testResult.proxy && (
                <span className={`text-xs ${testResult.proxy.success ? 'text-[var(--color-success)]' : 'text-[var(--color-error)]'}`}>
                  {testResult.proxy.success
                    ? t('settings.providers.proxyOk', { latency: String(testResult.proxy.latencyMs) })
                    : t('settings.providers.proxyFailed', { error: testResult.proxy.error || '' })}
                </span>
              )}
            </div>
          )}
        </div>
      </div>
    </Modal>
  )
}

function CodingModeSettings() {
  const codingMode = useSettingsStore((s) => s.codingMode)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const requestSetCodingMode = useSettingsStore((s) => s.requestSetCodingMode)
  const permissionMode = useSettingsStore((s) => s.permissionMode)
  const t = useTranslation()

  const MODE_GLYPH: Record<string, string> = {
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

  const modes = codingModes.length > 0 ? codingModes : []

  return (
    <div className="max-w-2xl">
      <h2 className="text-base font-semibold text-[var(--color-text-primary)] mb-1">
        {t('settings.codingMode.title')}
      </h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-4">
        {t('settings.codingMode.description')}
      </p>

      <div className="mb-3 px-3 py-2 rounded-lg bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
        <span className="material-symbols-outlined text-[14px]">shield</span>
        {t('settings.codingMode.derivedPermission', { mode: permissionMode })}
      </div>

      <div className="flex flex-col gap-2">
        {modes.map((m) => {
          const isSelected = codingMode === m.id
          return (
            <button
              key={m.id}
              onClick={() => void requestSetCodingMode(m.id)}
              className={`flex items-center gap-3 px-4 py-3 rounded-xl border transition-all text-left ${
                isSelected
                  ? 'border-[var(--color-brand)] bg-[var(--color-surface-container)] shadow-[var(--shadow-focus-ring)]'
                  : 'border-[var(--color-border)] hover:border-[var(--color-border-focus)] hover:bg-[var(--color-surface-hover)]'
              }`}
            >
              <span className="material-symbols-outlined text-[20px] text-[var(--color-text-secondary)]">
                {MODE_GLYPH[m.id] ?? 'tune'}
              </span>
              <div className="flex-1">
                <div className="text-xs font-semibold text-[var(--color-text-primary)] flex items-center gap-2">
                  {m.label}
                  <span className="text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded bg-[var(--color-surface-container-low)] text-[var(--color-text-tertiary)]">
                    {m.permissionMode}
                  </span>
                </div>
                <div className="text-xs text-[var(--color-text-tertiary)]">{m.description}</div>
              </div>
              {isSelected && (
                <span
                  className="material-symbols-outlined text-[18px] text-[var(--color-brand)]"
                  style={{ fontVariationSettings: "'FILL' 1" }}
                >
                  check_circle
                </span>
              )}
            </button>
          )
        })}
        {modes.length === 0 && (
          <div className="text-xs text-[var(--color-text-tertiary)] py-4 text-center">
            {t('settings.codingMode.loading')}
          </div>
        )}
      </div>
    </div>
  )
}

function GeneralSettings() {
  const effortLevel = useSettingsStore((s) => s.effortLevel)
  const setEffort = useSettingsStore((s) => s.setEffort)
  const locale = useSettingsStore((s) => s.locale)
  const setLocale = useSettingsStore((s) => s.setLocale)
  const theme = useSettingsStore((s) => s.theme)
  const setTheme = useSettingsStore((s) => s.setTheme)
  const t = useTranslation()

  const EFFORT_LABELS: Record<EffortLevel, string> = {
    low: t('settings.general.effort.low'),
    medium: t('settings.general.effort.medium'),
    high: t('settings.general.effort.high'),
    max: t('settings.general.effort.max'),
  }

  const LANGUAGES: Array<{ value: Locale; label: string }> = [
    { value: 'en', label: 'English' },
    { value: 'zh', label: '中文' },
  ]

  const THEMES: Array<{ value: ThemeMode; label: string }> = [
    { value: 'light', label: t('settings.general.appearance.light') },
    { value: 'dark', label: t('settings.general.appearance.dark') },
  ]

  return (
    <div className="max-w-xl">
      {}
      <h2 className="text-base font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.appearanceTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.appearanceDescription')}</p>
      <div className="flex gap-2 mb-8">
        {THEMES.map(({ value, label }) => (
          <button
            key={value}
            onClick={() => void setTheme(value)}
            className={`flex-1 h-9 text-xs font-semibold rounded-lg border transition-all ${
              theme === value
                ? 'bg-[image:var(--gradient-btn-primary)] text-[var(--color-btn-primary-fg)] border-transparent shadow-[var(--shadow-button-primary)]'
                : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {}
      <h2 className="text-base font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.languageTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.languageDescription')}</p>
      <div className="flex gap-2 mb-8">
        {LANGUAGES.map(({ value, label }) => (
          <button
            key={value}
            onClick={() => setLocale(value)}
            className={`flex-1 h-9 text-xs font-semibold rounded-lg border transition-all ${
              locale === value
                ? 'bg-[var(--color-brand)] text-white border-[var(--color-brand)]'
                : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {}
      <h2 className="text-base font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.effortTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.effortDescription')}</p>
      <div className="flex gap-2">
        {(['low', 'medium', 'high', 'max'] as EffortLevel[]).map((level) => (
          <button
            key={level}
            onClick={() => setEffort(level)}
            className={`flex-1 h-9 text-xs font-semibold rounded-lg border transition-all ${
              effortLevel === level
                ? 'bg-[var(--color-brand)] text-white border-[var(--color-brand)]'
                : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
            }`}
          >
            {EFFORT_LABELS[level]}
          </button>
        ))}
      </div>

    </div>
  )
}
