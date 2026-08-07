// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useEffect, useMemo, useRef, lazy, Suspense } from 'react'
import { useSettingsStore, PII_KIND_LABELS, type PiiKindLabel } from '../stores/settingsStore'
import { useChatStore } from '../stores/chatStore'
import { useTabStore } from '../stores/tabStore'
import { useAutonomyStore } from '../stores/autonomyStore'
import { useProviderStore } from '../stores/providerStore'
import { useTranslation, useCodingModeText, type TranslationKey } from '../i18n'
import { Modal } from '../components/shared/Modal'
import { ConfirmDialog } from '../components/shared/ConfirmDialog'
import { Input } from '../components/shared/Input'
import { Button } from '../components/shared/Button'
import type { EffortLevel, ThemeMode, CloseBehavior } from '../types/settings'
import type { Locale } from '../i18n'
import type {
  SavedProvider,
  UpdateProviderInput,
  ProviderTestResult,
  ApiFormat,
  CustomHttpHeader,
  ModelPricingEntry,
  DiscoveredModel,
} from '../types/provider'
import { normalizeApiFormat, apiFormatLabel } from '../types/provider'
import type { ProviderPreset } from '../types/providerPreset'
import type { CodingModeId } from '../types/codingMode'
import { sortByCodingModeOrder } from '../types/codingMode'
import { ApiError } from '../api/client'
import { settingsApi } from '../api/settings'
import {
  MODEL_TYPES,
  DEFAULT_MODEL_TYPE,
  effectiveModelTypes,
  sanitizeModelTypes,
  modelTypeLabelKey,
} from '../utils/modelTypes'
import { GlobalModelsPanel } from './GlobalModelsPanel'
import { ProviderModelsPanel } from './ProviderModelsPanel'
import { ModelDiscoveryPanel } from '../components/settings/ModelDiscoveryPanel'
import {
  NetworkProxySection,
  AutomationSection,
  SecuritySandboxSection,
} from '../components/settings/SystemSettingsSections'
import { usePluginStore } from '../stores/pluginStore'
import { useUIStore, type SettingsTab } from '../stores/uiStore'
import { useLanStore } from '../stores/lanStore'

const AdapterSettings = lazy(() =>
  import('./AdapterSettings').then((m) => ({ default: m.AdapterSettings })),
)
const ToolsAndMcpsSettings = lazy(() =>
  import('./ToolsAndMcpsSettings').then((m) => ({ default: m.ToolsAndMcpsSettings })),
)
const HooksSettings = lazy(() =>
  import('./HooksSettings').then((m) => ({ default: m.HooksSettings })),
)
const UsageSettings = lazy(() =>
  import('./UsageSettings').then((m) => ({ default: m.UsageSettings })),
)
const EvolutionSettings = lazy(() =>
  import('./EvolutionSettings').then((m) => ({ default: m.EvolutionSettings })),
)
const RulesSkillsSubagentsSettings = lazy(() =>
  import('./RulesSkillsSubagentsSettings').then((m) => ({
    default: m.RulesSkillsSubagentsSettings,
  })),
)
const AgentsSettings = lazy(() =>
  import('./AgentsSettings').then((m) => ({ default: m.AgentsSettings })),
)
const LspSettings = lazy(() =>
  import('./LspSettings').then((m) => ({ default: m.LspSettings })),
)
const KeyboardShortcutsSettings = lazy(() =>
  import('./KeyboardShortcutsSettings').then((m) => ({
    default: m.KeyboardShortcutsSettings,
  })),
)
const CredentialsSettings = lazy(() =>
  import('./CredentialsSettings').then((m) => ({ default: m.CredentialsSettings })),
)
const PluginList = lazy(() =>
  import('../components/plugins/PluginList').then((m) => ({ default: m.PluginList })),
)
const PluginDetail = lazy(() =>
  import('../components/plugins/PluginDetail').then((m) => ({ default: m.PluginDetail })),
)

function SettingsTabFallback() {
  return (
    <div className="flex items-center justify-center h-40 text-[var(--color-text-tertiary)]">
      <span className="material-symbols-outlined animate-spin text-[20px]">progress_activity</span>
    </div>
  )
}

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
            <TabButton icon="auto_awesome" label={t('settings.tab.evolution')} active={activeTab === 'evolution'} onClick={() => setActiveTab('evolution')} />
            <TabButton icon="dns" label={t('settings.tab.providers')} active={activeTab === 'providers'} onClick={() => setActiveTab('providers')} />
            <TabButton icon="smart_toy" label={t('settings.tab.agents')} active={activeTab === 'agents'} onClick={() => setActiveTab('agents')} />
            <TabButton icon="psychology" label={t('settings.tab.codingMode')} active={activeTab === 'codingMode'} onClick={() => setActiveTab('codingMode')} />
            <TabButton icon="policy" label={t('settings.tab.skills')} active={activeTab === 'skills'} onClick={() => setActiveTab('skills')} />
            <TabButton icon="build" label={t('settings.tab.mcp')} active={activeTab === 'mcp'} onClick={() => setActiveTab('mcp')} />
            <TabButton icon="extension" label={t('settings.tab.plugins')} active={activeTab === 'plugins'} onClick={() => setActiveTab('plugins')} />
            <TabButton icon="code" label={t('settings.tab.lsp')} active={activeTab === 'lsp'} onClick={() => setActiveTab('lsp')} />
            <TabButton icon="keyboard" label={t('settings.tab.keyboard')} active={activeTab === 'keyboard'} onClick={() => setActiveTab('keyboard')} />
            <TabButton icon="key" label={t('settings.tab.credentials')} active={activeTab === 'credentials'} onClick={() => setActiveTab('credentials')} />
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
          <Suspense fallback={<SettingsTabFallback />}>
            {activeTab === 'providers' && <ProviderSettings />}
            {activeTab === 'agents' && <AgentsSettings />}
            {activeTab === 'codingMode' && <CodingModeSettings />}
            {activeTab === 'general' && <GeneralSettings />}
            {activeTab === 'adapters' && <AdapterSettings />}
            {activeTab === 'mcp' && <ToolsAndMcpsSettings />}
            {activeTab === 'plugins' && <PluginsSettings />}
            {activeTab === 'lsp' && <LspSettings />}
            {activeTab === 'keyboard' && <KeyboardShortcutsSettings />}
            {activeTab === 'skills' && <RulesSkillsSubagentsSettings />}
            {activeTab === 'hooks' && <HooksSettings />}
            {activeTab === 'usage' && <UsageSettings />}
            {activeTab === 'evolution' && <EvolutionSettings />}
            {activeTab === 'credentials' && <CredentialsSettings />}
          </Suspense>
        </div>
      </div>
    </div>
  )
}

function PluginsSettings() {
  const selectedPlugin = usePluginStore((s) => s.selectedPlugin)
  return selectedPlugin ? <PluginDetail /> : <PluginList />
}

function SettingsSyncSection() {
  const t = useTranslation()
  const fileInputRef = useRef<HTMLInputElement>(null)
  const [isExporting, setIsExporting] = useState(false)
  const [isImporting, setIsImporting] = useState(false)
  const [message, setMessage] = useState<{ kind: 'ok' | 'error'; text: string } | null>(null)

  const handleExport = async () => {
    setMessage(null)
    setIsExporting(true)
    try {
      const res = await settingsApi.exportSyncSnapshot()
      const json = JSON.stringify(res.snapshot ?? {}, null, 2)
      const blob = new Blob([json], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      const stamp = new Date().toISOString().replace(/[:.]/g, '-')
      anchor.download = `senweavercoding-settings-${stamp}.json`
      document.body.appendChild(anchor)
      anchor.click()
      anchor.remove()
      URL.revokeObjectURL(url)
      setMessage({ kind: 'ok', text: t('settings.general.syncExported') })
    } catch (err) {
      setMessage({
        kind: 'error',
        text: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setIsExporting(false)
    }
  }

  const handleImportFile = async (file: File) => {
    setMessage(null)
    setIsImporting(true)
    try {
      const text = await file.text()
      const snapshot = JSON.parse(text)
      await settingsApi.importSyncSnapshot(snapshot)
      setMessage({ kind: 'ok', text: t('settings.general.syncImported') })
    } catch (err) {
      setMessage({
        kind: 'error',
        text: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setIsImporting(false)
    }
  }

  return (
    <div className="mt-6 border-t border-[var(--color-border)] pt-4">
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">
        {t('settings.general.syncTitle')}
      </h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">
        {t('settings.general.syncDescription')}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="secondary" size="sm" onClick={handleExport} loading={isExporting}>
          <span className="material-symbols-outlined text-[14px]">download</span>
          {t('settings.general.syncExport')}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => fileInputRef.current?.click()}
          loading={isImporting}
        >
          <span className="material-symbols-outlined text-[14px]">upload</span>
          {t('settings.general.syncImport')}
        </Button>
        <input
          ref={fileInputRef}
          type="file"
          accept="application/json,.json"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0]
            e.target.value = ''
            if (file) void handleImportFile(file)
          }}
        />
      </div>
      {message && (
        <div
          className={`mt-2 text-xs px-3 py-2 rounded-[var(--radius-md)] border ${
            message.kind === 'ok'
              ? 'border-[var(--color-success)]/30 bg-[var(--color-success)]/12 text-[var(--color-success)]'
              : 'border-[color:rgba(239,68,68,0.25)] bg-[color:rgba(239,68,68,0.08)] text-[var(--color-error)]'
          }`}
        >
          {message.text}
        </div>
      )}
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
  const fetchSettings = useSettingsStore((s) => s.fetchModels)
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
    setPendingDeleteProvider(provider)
  }

  const confirmDelete = async () => {
    if (!pendingDeleteProvider) return
    setIsDeletingProvider(true)
    try {
      await deleteProvider(pendingDeleteProvider.id)
      await fetchSettings()
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

  const refreshProviderModels = async () => {
    await fetchProviders()
    await fetchSettings()
  }

  const patchProvider = async (id: string, input: UpdateProviderInput) => {
    const saved = await updateProvider(id, input)
    await fetchSettings()
    return saved
  }

  const toggleExpanded = (id: string) => {
    setExpanded((prev) => ({ ...prev, [id]: !prev[id] }))
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-xs font-semibold text-[var(--color-text-primary)]">{t('settings.providers.title')}</h2>
          <p className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{t('settings.providers.description')}</p>
        </div>
        <Button size="sm" onClick={() => setShowCreateModal(true)} disabled={isPresetsLoading || presets.length === 0}>
          <span className="material-symbols-outlined text-[14px]">add</span>
          {t('settings.providers.addProvider')}
        </Button>
      </div>

      {providers.length > 0 && (
        <GlobalModelsPanel
          providers={providers}
          presetMap={presetMap}
          onRefresh={refreshProviderModels}
          onUpdateProvider={patchProvider}
        />
      )}

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
                      {provider.apiFormat && (
                        <span className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-warning)] leading-none">
                          {apiFormatLabel(provider.apiFormat)}
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
                    <Button variant="ghost" size="sm" onClick={() => handleDelete(provider)} className="text-[var(--color-error)] hover:text-[var(--color-error)]">{t('common.delete')}</Button>
                  </div>
                </div>

                {isExpanded && (
                  <div className="px-4 pb-3 pt-1 border-t border-[var(--color-border-separator)]">
                    <ProviderModelsPanel
                      provider={provider}
                      onUpdateProvider={patchProvider}
                      onEditModels={() => setEditingProvider(provider)}
                    />
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
    apiFormat: normalizeApiFormat(provider?.apiFormat ?? 'openai_chat'),
    defaultModels: provider?.models ?? [],
    needsApiKey: true,
    websiteUrl: '',
  }
}

function normalizeDisplayNameKey(value: string | null | undefined): string {
  if (!value) return ''
  return value
    .replace(/\s+/g, ' ')
    .trim()
    .toLowerCase()
}

function readErrorCode(body: unknown): string | null {
  if (body && typeof body === 'object' && 'code' in body) {
    const code = (body as { code?: unknown }).code
    if (typeof code === 'string' && code.length > 0) {
      return code
    }
  }
  return null
}

function isCustomPresetId(id: string | null | undefined): boolean {
  return (id ?? '').trim().toLowerCase() === 'custom'
}

function presetInitialName(preset: ProviderPreset): string {
  return isCustomPresetId(preset.id) ? '' : preset.name
}

function generateProviderEntityId(presetId: string): string {
  const sanitizedPresetSeed = (presetId || 'provider')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/(^-|-$)/g, '')
  const seed = sanitizedPresetSeed.length > 0 ? sanitizedPresetSeed : 'provider'
  let unique = ''
  try {
    const cryptoApi = typeof globalThis !== 'undefined' ? globalThis.crypto : undefined
    if (cryptoApi && typeof cryptoApi.randomUUID === 'function') {
      unique = cryptoApi.randomUUID().replace(/-/g, '').slice(0, 12)
    }
  } catch {

  }
  if (!unique) {
    unique = `${Date.now().toString(36)}${Math.floor(Math.random() * 1_000_000).toString(36)}`
  }
  return `${seed}-${unique}`
}

type ModelRow = { id: string; value: string }

function createModelRowId(): string {
  try {
    const cryptoApi = typeof globalThis !== 'undefined' ? globalThis.crypto : undefined
    if (cryptoApi && typeof cryptoApi.randomUUID === 'function') {
      return cryptoApi.randomUUID()
    }
  } catch {

  }
  return `${Date.now().toString(36)}${Math.floor(Math.random() * 1_000_000).toString(36)}`
}

function toModelRows(values: string[]): ModelRow[] {
  return values.map((value) => ({ id: createModelRowId(), value }))
}

function ModelTypeSelect({
  selected,
  onToggle,
  disabled,
  t,
}: {
  selected: string[]
  onToggle: (type: string) => void
  disabled: boolean
  t: ReturnType<typeof useTranslation>
}) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    return () => document.removeEventListener('mousedown', onDoc)
  }, [open])
  const eff = effectiveModelTypes(selected)
  const summary = eff
    .map((tpe) => t(modelTypeLabelKey(tpe) as TranslationKey))
    .join('、')
  return (
    <div ref={ref} className="relative flex-shrink-0">
      <button
        type="button"
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        title={t('settings.providers.modelTypeTooltip')}
        className="flex w-40 items-center gap-1 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-2 py-1.5 text-xs text-[var(--color-text-primary)] outline-none transition-colors hover:border-[var(--color-border-focus)] disabled:opacity-50"
      >
        <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
          category
        </span>
        <span className="min-w-0 flex-1 truncate text-left">{summary}</span>
        <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
          expand_more
        </span>
      </button>
      {open && (
        <div className="absolute right-0 top-full z-[9999] mt-1 w-56 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] py-1 shadow-[var(--shadow-dropdown)]">
          <div className="px-3 pb-1 pt-0.5 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
            {t('settings.providers.modelTypeLabel')}
          </div>
          {MODEL_TYPES.map((tpe) => {
            const on = eff.includes(tpe)
            return (
              <button
                key={tpe}
                type="button"
                onClick={() => onToggle(tpe)}
                className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors hover:bg-[var(--color-surface-hover)] ${
                  on ? 'text-[var(--color-brand)]' : 'text-[var(--color-text-primary)]'
                }`}
              >
                <span className="material-symbols-outlined text-[16px]">
                  {on ? 'check_box' : 'check_box_outline_blank'}
                </span>
                <span className="truncate">{t(modelTypeLabelKey(tpe) as TranslationKey)}</span>
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}

function ProviderFormModal({ open, onClose, mode, provider, presets }: ProviderFormProps) {
  const createProvider = useProviderStore((s) => s.createProvider)
  const updateProvider = useProviderStore((s) => s.updateProvider)
  const testConfig = useProviderStore((s) => s.testConfig)
  const allProviders = useProviderStore((s) => s.providers)
  const fetchSettings = useSettingsStore((s) => s.fetchModels)
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
  const [name, setName] = useState(provider?.name ?? presetInitialName(initialPreset))
  const [baseUrl, setBaseUrl] = useState(provider?.baseUrl ?? initialPreset.baseUrl)
  const [apiFormat, setApiFormat] = useState<ApiFormat>(
    normalizeApiFormat(provider?.apiFormat ?? initialPreset.apiFormat ?? 'openai_chat'),
  )
  const [apiKey, setApiKey] = useState('')
  const [notes, setNotes] = useState(provider?.notes ?? '')
  const [modelRows, setModelRows] = useState<ModelRow[]>(() =>
    toModelRows(provider?.models ?? [...initialPreset.defaultModels]),
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
  const [modelTypes, setModelTypes] = useState<Record<string, string[]>>(() => {
    const initial: Record<string, string[]> = {}
    if (provider?.modelTypes) {
      for (const [key, value] of Object.entries(provider.modelTypes)) {
        const sanitized = sanitizeModelTypes(value)
        if (sanitized.length > 0) initial[key] = sanitized
      }
    }
    return initial
  })
  const [modelPricing, setModelPricing] = useState<
    Record<string, { input: string; output: string }>
  >(() => {
    const initial: Record<string, { input: string; output: string }> = {}
    if (provider?.modelPricing) {
      for (const [key, value] of Object.entries(provider.modelPricing)) {
        const input =
          typeof value?.input === 'number' && Number.isFinite(value.input) && value.input > 0
            ? String(value.input)
            : ''
        const output =
          typeof value?.output === 'number' && Number.isFinite(value.output) && value.output > 0
            ? String(value.output)
            : ''
        if (input || output) initial[key] = { input, output }
      }
    }
    return initial
  })
  const [customHeaders, setCustomHeaders] = useState<CustomHttpHeader[]>(() => {
    if (!provider?.customHeaders) return []
    return provider.customHeaders.map((header) => ({
      name: header.name ?? '',
      value: header.value ?? '',
      enabled: typeof header.enabled === 'boolean' ? header.enabled : true,
    }))
  })
  const [advancedExpanded, setAdvancedExpanded] = useState(() =>
    (provider?.customHeaders?.length ?? 0) > 0,
  )
  const [visibleHeaderValues, setVisibleHeaderValues] = useState<Set<number>>(() => new Set())
  const [newModelDraft, setNewModelDraft] = useState('')
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [testResult, setTestResult] = useState<ProviderTestResult | null>(null)
  const [isTesting, setIsTesting] = useState(false)

  const normalizedName = useMemo(() => normalizeDisplayNameKey(name), [name])
  const duplicateProvider = useMemo(() => {
    if (!normalizedName) return null
    return (
      allProviders.find((existing) => {
        if (mode === 'edit' && provider && existing.id === provider.id) return false
        return normalizeDisplayNameKey(existing.name) === normalizedName
      }) ?? null
    )
  }, [allProviders, normalizedName, mode, provider])

  const handlePresetChange = (preset: ProviderPreset) => {
    if (preset.id === selectedPreset.id) {

      return
    }
    setSelectedPreset(preset)
    setName(presetInitialName(preset))
    setBaseUrl(preset.baseUrl)
    setApiFormat(normalizeApiFormat(preset.apiFormat ?? 'openai_chat'))
    setModelRows(toModelRows([...preset.defaultModels]))

    setModelContextWindows({})
    setModelTypes({})
    setModelPricing({})
    setTestResult(null)
    setSubmitError(null)
  }

  const isCustom = selectedPreset.id === 'custom'
  const trimmedModels = useMemo(
    () =>
      Array.from(
        new Set(
          modelRows
            .map((row) => row.value.trim())
            .filter((m) => m.length > 0),
        ),
      ),
    [modelRows],
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
    setModelRows((prev) =>
      prev.some((row) => row.value.trim() === candidate)
        ? prev
        : [...prev, { id: createModelRowId(), value: candidate }],
    )
    setNewModelDraft('')
  }

  const applyDiscoveredModels = (discovered: DiscoveredModel[]) => {
    const existingIds = new Set(modelRows.map((row) => row.value.trim()).filter(Boolean))
    const newOnes = discovered.filter((model) => {
      const id = model.id.trim()
      return id.length > 0 && !existingIds.has(id)
    })
    if (newOnes.length === 0) return
    setModelRows((prev) => [
      ...prev,
      ...newOnes.map((model) => ({ id: createModelRowId(), value: model.id.trim() })),
    ])
    setModelTypes((prev) => {
      const next = { ...prev }
      for (const model of newOnes) {
        const id = model.id.trim()
        const sanitized = sanitizeModelTypes(model.types)
        if (sanitized.length > 0) next[id] = sanitized
      }
      return next
    })
  }

  const removeModel = (rowId: string) => {
    setModelRows((prev) => {
      const removed = prev.find((row) => row.id === rowId)
      if (removed) {
        const modelId = removed.value.trim()
        if (modelId) {
          setModelContextWindows((windows) => {
            if (!(modelId in windows)) return windows
            const { [modelId]: _removed, ...rest } = windows
            return rest
          })
          setModelTypes((types) => {
            if (!(modelId in types)) return types
            const { [modelId]: _removed, ...rest } = types
            return rest
          })
          setModelPricing((pricing) => {
            if (!(modelId in pricing)) return pricing
            const { [modelId]: _removed, ...rest } = pricing
            return rest
          })
        }
      }
      return prev.filter((row) => row.id !== rowId)
    })
  }

  const updateModel = (rowId: string, value: string) => {
    setModelRows((prev) => {
      const previous = prev.find((row) => row.id === rowId)?.value
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
        setModelTypes((existing) => {
          const carried = existing[previous]
          if (carried === undefined) return existing
          const { [previous]: _removed, ...rest } = existing
          const trimmedNext = value.trim()
          if (!trimmedNext) return rest
          const merged: Record<string, string[]> = { ...rest }
          merged[trimmedNext] = carried
          return merged
        })
        setModelPricing((existing) => {
          const carried = existing[previous]
          if (carried === undefined) return existing
          const { [previous]: _removed, ...rest } = existing
          const trimmedNext = value.trim()
          if (!trimmedNext) return rest
          const merged: Record<string, { input: string; output: string }> = { ...rest }
          merged[trimmedNext] = carried
          return merged
        })
      }
      return prev.map((row) => (row.id === rowId ? { ...row, value } : row))
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

  const updateModelPricing = (
    modelId: string,
    field: 'input' | 'output',
    raw: string,
  ) => {
    setModelPricing((prev) => {
      const sanitized = raw.replace(/[^0-9.]/g, '')
      const current = prev[modelId] ?? { input: '', output: '' }
      const next = { ...current, [field]: sanitized }
      if (!next.input && !next.output) {
        if (!(modelId in prev)) return prev
        const { [modelId]: _removed, ...rest } = prev
        return rest
      }
      return { ...prev, [modelId]: next }
    })
  }

  const buildModelPricingPayload = (): Record<string, ModelPricingEntry> => {
    const payload: Record<string, ModelPricingEntry> = {}
    const seen = new Set(trimmedModels)
    for (const [model, raw] of Object.entries(modelPricing)) {
      if (!seen.has(model)) continue
      const input = Number.parseFloat(raw.input)
      const output = Number.parseFloat(raw.output)
      const inputOk = Number.isFinite(input) && input > 0
      const outputOk = Number.isFinite(output) && output > 0
      if (!inputOk && !outputOk) continue
      payload[model] = {
        input: inputOk ? input : 0,
        output: outputOk ? output : 0,
      }
    }
    return payload
  }

  const toggleModelType = (modelId: string, type: string) => {
    if (!modelId) return
    setModelTypes((prev) => {
      const current = effectiveModelTypes(prev[modelId])
      const next = current.includes(type as never)
        ? current.filter((t) => t !== type)
        : [...current, type]
      const sanitized = sanitizeModelTypes(next)
      const resolved = sanitized.length > 0 ? sanitized : [DEFAULT_MODEL_TYPE]
      return { ...prev, [modelId]: resolved }
    })
  }

  const buildModelTypesPayload = (): Record<string, string[]> => {
    const payload: Record<string, string[]> = {}
    const seen = new Set(trimmedModels)
    for (const model of trimmedModels) {
      if (!seen.has(model)) continue
      const sanitized = sanitizeModelTypes(modelTypes[model])
      if (sanitized.length === 0) continue
      const isDefaultOnly =
        sanitized.length === 1 && sanitized[0] === DEFAULT_MODEL_TYPE
      if (isDefaultOnly) continue
      payload[model] = sanitized
    }
    return payload
  }

  const buildCustomHeadersPayload = (): CustomHttpHeader[] => {
    return customHeaders
      .map((entry) => ({
        name: entry.name.trim(),
        value: entry.value,
        enabled: entry.enabled,
      }))
      .filter((entry) => entry.name.length > 0)
  }

  const addCustomHeader = () => {
    setCustomHeaders((prev) => [...prev, { name: '', value: '', enabled: true }])
  }

  const updateCustomHeaderField = (
    index: number,
    field: 'name' | 'value' | 'enabled',
    nextValue: string | boolean,
  ) => {
    setCustomHeaders((prev) =>
      prev.map((entry, i) => {
        if (i !== index) return entry
        if (field === 'enabled' && typeof nextValue === 'boolean') {
          return { ...entry, enabled: nextValue }
        }
        if (field === 'name' && typeof nextValue === 'string') {
          return { ...entry, name: nextValue }
        }
        if (field === 'value' && typeof nextValue === 'string') {
          return { ...entry, value: nextValue }
        }
        return entry
      }),
    )
  }

  const removeCustomHeader = (index: number) => {
    setCustomHeaders((prev) => prev.filter((_, i) => i !== index))
    setVisibleHeaderValues((prev) => {
      if (!prev.has(index)) return prev
      const next = new Set<number>()
      prev.forEach((idx) => {
        if (idx < index) next.add(idx)
        else if (idx > index) next.add(idx - 1)
      })
      return next
    })
  }

  const toggleCustomHeaderValueVisibility = (index: number) => {
    setVisibleHeaderValues((prev) => {
      const next = new Set(prev)
      if (next.has(index)) {
        next.delete(index)
      } else {
        next.add(index)
      }
      return next
    })
  }

  const handleSubmit = async () => {
    if (!canSubmit) return
    setSubmitError(null)
    setIsSubmitting(true)
    try {
      const overridesPayload = buildContextWindowsPayload()
      const modelTypesPayload = buildModelTypesPayload()
      const modelPricingPayload = buildModelPricingPayload()
      const customHeadersPayload = buildCustomHeadersPayload()
      if (mode === 'create') {
        await createProvider({
          id: generateProviderEntityId(selectedPreset.id),
          presetId: selectedPreset.id,
          name: name.trim(),
          apiKey: apiKey.trim(),
          baseUrl: baseUrl.trim(),
          apiFormat,
          models: trimmedModels,
          modelContextWindows: overridesPayload,
          modelTypes: modelTypesPayload,
          modelPricing: modelPricingPayload,
          customHeaders: customHeadersPayload,
          notes: notes.trim() || undefined,
        })
      } else if (provider) {
        const input: UpdateProviderInput = {
          name: name.trim(),
          baseUrl: baseUrl.trim(),
          apiFormat,
          models: trimmedModels,
          modelContextWindows: overridesPayload,
          modelTypes: modelTypesPayload,
          modelPricing: modelPricingPayload,
          customHeaders: customHeadersPayload,
          notes: notes.trim() || undefined,
        }
        if (apiKey.trim()) input.apiKey = apiKey.trim()
        await updateProvider(provider.id, input)
      }
      await fetchSettings()
      onClose()
    } catch (err) {
      console.error('Failed to save provider:', err)
      const apiErr = err as ApiError | Error
      const code = apiErr instanceof ApiError ? readErrorCode(apiErr.body) : null
      if (code === 'name_conflict') {
        setSubmitError(t('settings.providers.nameConflict'))
      } else {
        const fallback = apiErr instanceof Error ? apiErr.message : String(err)
        setSubmitError(fallback || t('settings.providers.requestFailed'))
      }
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
          <Button variant="secondary" size="sm" onClick={onClose}>{t('common.cancel')}</Button>
          <Button size="sm" onClick={handleSubmit} disabled={!canSubmit} loading={isSubmitting}>
            {mode === 'create' ? t('common.add') : t('common.save')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-xs">
        {}
        {mode === 'create' && (
          <div>
            <label className="text-xs font-medium text-[var(--color-text-primary)] mb-2 block">{t('settings.providers.preset')}</label>
            <div className="flex flex-wrap gap-2">
              {availablePresets.map((preset) => (
                <button
                  key={preset.id}
                  onClick={() => handlePresetChange(preset)}
                  className={`px-2.5 py-1 text-xs font-medium rounded-full border transition-all ${
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

        <div>
          <Input
            label={t('settings.providers.name')}
            required
            value={name}
            onChange={(e) => {
              setName(e.target.value)
              setSubmitError(null)
            }}
            placeholder={t('settings.providers.namePlaceholder')}
          />
          {duplicateProvider ? (
            <div className="mt-1 text-xs text-[var(--color-error)]">
              {t('settings.providers.nameConflictHint').replace(
                '{{name}}',
                duplicateProvider.name,
              )}
            </div>
          ) : null}
        </div>

        {submitError ? (
          <div className="text-xs text-[var(--color-error)] px-3 py-2 rounded-[var(--radius-md)] bg-[color:rgba(239,68,68,0.08)] border border-[color:rgba(239,68,68,0.25)]">
            {submitError}
          </div>
        ) : null}

        <Input label={t('settings.providers.notes')} value={notes} onChange={(e) => setNotes(e.target.value)} placeholder={t('settings.providers.notesPlaceholder')} />

        {}
        {isCustom || mode === 'edit' ? (
          <Input label={t('settings.providers.baseUrl')} required value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder={t('settings.providers.baseUrlPlaceholder')} />
        ) : (
          <div>
            <label className="text-xs font-medium text-[var(--color-text-primary)] mb-1 block">{t('settings.providers.baseUrl')}</label>
            <div className="text-xs text-[var(--color-text-tertiary)] px-2.5 py-1.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)]">
              {baseUrl}
            </div>
          </div>
        )}

        {}
        <div>
          <label className="text-xs font-medium text-[var(--color-text-primary)] mb-1 block">{t('settings.providers.apiFormat')}</label>
          <select
            value={apiFormat}
            onChange={(e) => setApiFormat(normalizeApiFormat(e.target.value))}
            className="w-full text-xs px-2.5 py-1.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
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
            <span className="ml-2 text-xs font-normal text-[var(--color-text-tertiary)]">{t('settings.providers.modelsHelp')}</span>
          </label>
          <div className="flex flex-col gap-1.5">
            {modelRows.length === 0 && (
              <div className="text-xs italic text-[var(--color-text-tertiary)] px-3 py-2 rounded-md border border-dashed border-[var(--color-border)]">
                {t('settings.providers.noModels')}
              </div>
            )}
            {modelRows.map((row, idx) => {
              const trimmedModelId = row.value.trim()
              const contextWindowDraft = trimmedModelId
                ? modelContextWindows[trimmedModelId] ?? ''
                : ''
              const pricingDraft = trimmedModelId
                ? modelPricing[trimmedModelId] ?? { input: '', output: '' }
                : { input: '', output: '' }
              return (
                <div key={row.id} className="flex items-center gap-2">
                  <input
                    value={row.value}
                    onChange={(e) => updateModel(row.id, e.target.value)}
                    placeholder="model-id"
                    className="flex-1 text-xs font-mono px-2.5 py-1.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
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
                    className="w-28 text-xs font-mono px-2 py-1.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50"
                  />
                  <input
                    value={pricingDraft.input}
                    onChange={(e) =>
                      trimmedModelId &&
                      updateModelPricing(trimmedModelId, 'input', e.target.value)
                    }
                    inputMode="decimal"
                    placeholder={t('settings.providers.priceInputPlaceholder')}
                    title={t('settings.providers.priceInputTooltip')}
                    disabled={!trimmedModelId}
                    className="w-16 text-xs font-mono px-2 py-1.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50"
                  />
                  <input
                    value={pricingDraft.output}
                    onChange={(e) =>
                      trimmedModelId &&
                      updateModelPricing(trimmedModelId, 'output', e.target.value)
                    }
                    inputMode="decimal"
                    placeholder={t('settings.providers.priceOutputPlaceholder')}
                    title={t('settings.providers.priceOutputTooltip')}
                    disabled={!trimmedModelId}
                    className="w-16 text-xs font-mono px-2 py-1.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50"
                  />
                  <ModelTypeSelect
                    selected={trimmedModelId ? modelTypes[trimmedModelId] ?? [] : []}
                    onToggle={(type) => toggleModelType(trimmedModelId, type)}
                    disabled={!trimmedModelId}
                    t={t}
                  />
                  {idx === 0 && (
                    <span className="px-1.5 py-0.5 text-[10px] font-bold rounded border border-[var(--color-brand)]/18 bg-[var(--color-brand)]/14 text-[var(--color-brand)] leading-none">
                      {t('settings.providers.primaryTag')}
                    </span>
                  )}
                  <button
                    onClick={() => removeModel(row.id)}
                    className="text-[var(--color-text-tertiary)] hover:text-[var(--color-error)] flex-shrink-0"
                    title={t('common.delete')}
                  >
                    <span className="material-symbols-outlined text-[18px]">close</span>
                  </button>
                </div>
              )
            })}
            <ModelDiscoveryPanel
              baseUrl={baseUrl}
              apiFormat={apiFormat}
              apiKey={apiKey}
              presetId={selectedPreset.id}
              providerId={provider?.id}
              existingModelIds={trimmedModels}
              onApply={applyDiscoveredModels}
            />
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
                className="flex-1 text-xs font-mono px-2.5 py-1.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-dashed border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
              />
              <Button variant="secondary" size="sm" onClick={() => addModel()} disabled={!newModelDraft.trim()}>
                <span className="material-symbols-outlined text-[14px]">add</span>
                {t('settings.providers.addModel')}
              </Button>
            </div>
          </div>
        </div>

        <AdvancedSettingsSection
          presetId={selectedPreset.id}
          expanded={advancedExpanded}
          onToggleExpanded={() => setAdvancedExpanded((v) => !v)}
          customHeaders={customHeaders}
          visibleHeaderValues={visibleHeaderValues}
          onAddHeader={addCustomHeader}
          onRemoveHeader={removeCustomHeader}
          onUpdateHeader={updateCustomHeaderField}
          onToggleHeaderVisibility={toggleCustomHeaderValueVisibility}
        />

        {}
        <div className="flex items-center gap-3">
          <Button variant="secondary" size="sm" onClick={handleTest} loading={isTesting} disabled={!baseUrl.trim() || !primaryModel}>
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

const DISALLOWED_CUSTOM_HEADER_NAMES = new Set([
  'content-type',
  'content-length',
  'host',
  'authorization',
  'transfer-encoding',
  'connection',
  'proxy-authorization',
])

const VALID_HEADER_NAME_RE = /^[A-Za-z0-9!#$%&'*+\-.^_`|~]+$/

function validateHeaderName(raw: string): {
  invalid: boolean
  disallowed: boolean
} {
  const trimmed = raw.trim()
  if (trimmed.length === 0) {
    return { invalid: false, disallowed: false }
  }
  if (DISALLOWED_CUSTOM_HEADER_NAMES.has(trimmed.toLowerCase())) {
    return { invalid: false, disallowed: true }
  }
  if (!VALID_HEADER_NAME_RE.test(trimmed)) {
    return { invalid: true, disallowed: false }
  }
  return { invalid: false, disallowed: false }
}

type CustomHeaderPlaceholder = { name: string; value: string }

function customHeaderPlaceholdersForPreset(presetId: string): CustomHeaderPlaceholder {
  switch (presetId) {
    case 'openrouter':
      return { name: 'HTTP-Referer', value: 'https://your-app.example' }
    case 'anthropic':
      return { name: 'anthropic-beta', value: 'prompt-caching-2024-07-31' }
    case 'openai':
    case 'openai-codex':
      return { name: 'OpenAI-Beta', value: 'assistants=v2' }
    case 'gemini':
      return { name: 'x-goog-user-project', value: 'your-gcp-project' }
    default:
      return { name: 'x-custom-header', value: 'value' }
  }
}

type AdvancedSettingsSectionProps = {
  presetId: string
  expanded: boolean
  onToggleExpanded: () => void
  customHeaders: CustomHttpHeader[]
  visibleHeaderValues: Set<number>
  onAddHeader: () => void
  onRemoveHeader: (index: number) => void
  onUpdateHeader: (
    index: number,
    field: 'name' | 'value' | 'enabled',
    value: string | boolean,
  ) => void
  onToggleHeaderVisibility: (index: number) => void
}

function AdvancedSettingsSection({
  presetId,
  expanded,
  onToggleExpanded,
  customHeaders,
  visibleHeaderValues,
  onAddHeader,
  onRemoveHeader,
  onUpdateHeader,
  onToggleHeaderVisibility,
}: AdvancedSettingsSectionProps) {
  const t = useTranslation()
  const placeholder = customHeaderPlaceholdersForPreset(presetId)

  const lowerNameCounts = useMemo(() => {
    const counts = new Map<string, number>()
    for (const entry of customHeaders) {
      const key = entry.name.trim().toLowerCase()
      if (key.length === 0) continue
      counts.set(key, (counts.get(key) ?? 0) + 1)
    }
    return counts
  }, [customHeaders])

  return (
    <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
      <button
        type="button"
        onClick={onToggleExpanded}
        className="w-full flex items-center justify-between px-3 py-2 text-xs font-medium text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)] rounded-t-[var(--radius-md)]"
      >
        <span className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[16px]">
            {expanded ? 'expand_more' : 'chevron_right'}
          </span>
          {t('settings.providers.advanced.title')}
        </span>
      </button>
      {expanded && (
        <div className="px-3 pb-3 pt-1 border-t border-[var(--color-border-separator)] flex flex-col gap-3">
          <div>
            <div className="text-xs font-medium text-[var(--color-text-primary)]">
              {t('settings.providers.advanced.customHeaders.title')}
            </div>
            <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
              {t('settings.providers.advanced.customHeaders.description')}
            </div>
          </div>

          {customHeaders.length === 0 ? (
            <div className="text-xs italic text-[var(--color-text-tertiary)] px-3 py-2 rounded-md border border-dashed border-[var(--color-border)]">
              {t('settings.providers.advanced.customHeaders.empty')}
            </div>
          ) : (
            <div className="flex flex-col gap-1.5">
              {customHeaders.map((entry, index) => {
                const nameStatus = validateHeaderName(entry.name)
                const trimmedLower = entry.name.trim().toLowerCase()
                const isDuplicate =
                  trimmedLower.length > 0 && (lowerNameCounts.get(trimmedLower) ?? 0) > 1
                const showValue = visibleHeaderValues.has(index)
                return (
                  <div key={`header-${index}`} className="flex flex-col gap-1">
                    <div className="flex items-center gap-2">
                      <input
                        value={entry.name}
                        onChange={(e) => onUpdateHeader(index, 'name', e.target.value)}
                        placeholder={
                          placeholder.name ||
                          t('settings.providers.advanced.customHeaders.nameLabel')
                        }
                        aria-label={t('settings.providers.advanced.customHeaders.nameLabel')}
                        className="flex-1 min-w-0 text-xs font-mono px-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
                      />
                      <div className="relative flex-1 min-w-0">
                        <input
                          value={entry.value}
                          onChange={(e) => onUpdateHeader(index, 'value', e.target.value)}
                          type={showValue ? 'text' : 'password'}
                          placeholder={
                            placeholder.value ||
                            t('settings.providers.advanced.customHeaders.valueLabel')
                          }
                          aria-label={t('settings.providers.advanced.customHeaders.valueLabel')}
                          className="w-full text-xs font-mono pr-9 pl-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
                        />
                        <button
                          type="button"
                          onClick={() => onToggleHeaderVisibility(index)}
                          className="absolute right-1.5 top-1/2 -translate-y-1/2 p-1 rounded text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
                          title={
                            showValue
                              ? t('settings.providers.advanced.customHeaders.hide')
                              : t('settings.providers.advanced.customHeaders.show')
                          }
                          aria-label={
                            showValue
                              ? t('settings.providers.advanced.customHeaders.hide')
                              : t('settings.providers.advanced.customHeaders.show')
                          }
                        >
                          <span className="material-symbols-outlined text-[16px]">
                            {showValue ? 'visibility_off' : 'visibility'}
                          </span>
                        </button>
                      </div>
                      <label className="flex items-center gap-1 text-xs text-[var(--color-text-secondary)] flex-shrink-0">
                        <input
                          type="checkbox"
                          checked={entry.enabled}
                          onChange={(e) => onUpdateHeader(index, 'enabled', e.target.checked)}
                          className="h-3.5 w-3.5"
                        />
                        {t('settings.providers.advanced.customHeaders.enabled')}
                      </label>
                      <button
                        type="button"
                        onClick={() => onRemoveHeader(index)}
                        className="text-[var(--color-text-tertiary)] hover:text-[var(--color-error)] flex-shrink-0 p-1"
                        title={t('settings.providers.advanced.customHeaders.remove')}
                        aria-label={t('settings.providers.advanced.customHeaders.remove')}
                      >
                        <span className="material-symbols-outlined text-[18px]">delete</span>
                      </button>
                    </div>
                    {(nameStatus.invalid || nameStatus.disallowed || isDuplicate) && (
                      <div className="text-xs text-[var(--color-warning)] pl-1">
                        {nameStatus.disallowed
                          ? t('settings.providers.advanced.customHeaders.disallowed')
                          : nameStatus.invalid
                            ? t('settings.providers.advanced.customHeaders.invalidName')
                            : t('settings.providers.advanced.customHeaders.duplicateName')}
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
          )}

          <div className="flex">
            <Button variant="secondary" size="sm" onClick={onAddHeader}>
              <span className="material-symbols-outlined text-[14px]">add</span>
              {t('settings.providers.advanced.customHeaders.add')}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

function CodingModeSettings() {
  const codingMode = useSettingsStore((s) => s.codingMode)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const codingModeOrder = useSettingsStore((s) => s.codingModeOrder)
  const setCodingModeOrder = useSettingsStore((s) => s.setCodingModeOrder)
  const requestSetCodingMode = useSettingsStore((s) => s.requestSetCodingMode)
  const permissionMode = useSettingsStore((s) => s.permissionMode)
  const t = useTranslation()
  const tCodingMode = useCodingModeText()

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

  const modes = useMemo(
    () => sortByCodingModeOrder(codingModes, codingModeOrder),
    [codingModes, codingModeOrder],
  )

  const [dragId, setDragId] = useState<CodingModeId | null>(null)
  const [dragOverId, setDragOverId] = useState<CodingModeId | null>(null)
  const dragStateRef = useRef<{
    id: CodingModeId
    startX: number
    startY: number
    pointerId: number
    active: boolean
  } | null>(null)
  const suppressClickRef = useRef(false)

  const modeIds = modes.map((m) => m.id)

  const findModeAtPoint = (x: number, y: number): CodingModeId | null => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null
    const row = el?.closest('[data-coding-mode-row]') as HTMLElement | null
    const id = row?.getAttribute('data-coding-mode-row')
    return id && modeIds.includes(id as CodingModeId) ? (id as CodingModeId) : null
  }

  const reorder = (sourceId: CodingModeId, targetId: CodingModeId) => {
    if (sourceId === targetId) return
    const fromIdx = modeIds.indexOf(sourceId)
    const toIdx = modeIds.indexOf(targetId)
    if (fromIdx === -1 || toIdx === -1) return
    const next = [...modeIds]
    next.splice(fromIdx, 1)
    next.splice(toIdx, 0, sourceId)
    setCodingModeOrder(next)
  }

  const handlePointerDown = (e: React.PointerEvent, id: CodingModeId) => {
    if (e.button !== 0) return
    suppressClickRef.current = false
    dragStateRef.current = {
      id,
      startX: e.clientX,
      startY: e.clientY,
      pointerId: e.pointerId,
      active: false,
    }
  }

  const handlePointerMove = (e: React.PointerEvent) => {
    const st = dragStateRef.current
    if (!st) return
    if (!st.active) {
      const dx = e.clientX - st.startX
      const dy = e.clientY - st.startY
      if (Math.hypot(dx, dy) < 5) return
      st.active = true
      setDragId(st.id)
      try {
        ;(e.currentTarget as HTMLElement).setPointerCapture(st.pointerId)
      } catch {
      }
    }
    const targetId = findModeAtPoint(e.clientX, e.clientY)
    setDragOverId(targetId && targetId !== st.id ? targetId : null)
  }

  const handlePointerUp = (e: React.PointerEvent) => {
    const st = dragStateRef.current
    dragStateRef.current = null
    if (!st) return
    try {
      ;(e.currentTarget as HTMLElement).releasePointerCapture(st.pointerId)
    } catch {
    }
    if (st.active) {
      suppressClickRef.current = true
      const targetId = findModeAtPoint(e.clientX, e.clientY)
      setDragId(null)
      setDragOverId(null)
      if (targetId) reorder(st.id, targetId)
    }
  }

  return (
    <div>
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">
        {t('settings.codingMode.title')}
      </h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-2">
        {t('settings.codingMode.description')}
      </p>

      <div className="mb-3 px-3 py-2 rounded-lg bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
        <span className="material-symbols-outlined text-[14px]">drag_indicator</span>
        {t('settings.codingMode.reorderHint')}
      </div>

      <div className="mb-3 px-3 py-2 rounded-lg bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
        <span className="material-symbols-outlined text-[14px]">shield</span>
        {t('settings.codingMode.derivedPermission', { mode: permissionMode })}
      </div>

      <div className="flex flex-col gap-2">
        {modes.map((m) => {
          const isSelected = codingMode === m.id
          const isDragging = dragId === m.id
          const isDragOver = dragOverId === m.id && dragId !== null && dragId !== m.id
          return (
            <div
              key={m.id}
              role="button"
              tabIndex={0}
              data-coding-mode-row={m.id}
              onClick={() => {
                if (suppressClickRef.current) {
                  suppressClickRef.current = false
                  return
                }
                void requestSetCodingMode(m.id)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault()
                  void requestSetCodingMode(m.id)
                }
              }}
              onPointerDown={(e) => handlePointerDown(e, m.id)}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              style={{ touchAction: 'none' }}
              className={`flex items-center gap-2 px-3 py-2.5 rounded-xl border cursor-pointer select-none transition-all text-left ${
                isSelected
                  ? 'border-[var(--color-brand)] bg-[var(--color-surface-container)] shadow-[var(--shadow-focus-ring)]'
                  : 'border-[var(--color-border)] hover:border-[var(--color-border-focus)] hover:bg-[var(--color-surface-hover)]'
              }${isDragging ? ' opacity-50' : ''}${
                isDragOver ? ' border-[var(--color-brand)] border-dashed' : ''
              }`}
            >
              <span
                className="material-symbols-outlined text-[18px] text-[var(--color-text-tertiary)] cursor-grab active:cursor-grabbing shrink-0"
                title={t('settings.codingMode.dragHandle')}
              >
                drag_indicator
              </span>
              <span className="material-symbols-outlined text-[18px] text-[var(--color-text-secondary)] shrink-0">
                {MODE_GLYPH[m.id] ?? 'tune'}
              </span>
              <div className="flex-1 min-w-0">
                <div className="text-xs font-semibold text-[var(--color-text-primary)] flex items-center gap-2">
                  {tCodingMode(m.id, 'label', m.label)}
                  <span className="text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded bg-[var(--color-surface-container-low)] text-[var(--color-text-tertiary)]">
                    {m.permissionMode}
                  </span>
                </div>
                <div className="text-xs text-[var(--color-text-tertiary)]">
                  {tCodingMode(m.id, 'description', m.description ?? '')}
                </div>
              </div>
              {isSelected && (
                <span
                  className="material-symbols-outlined text-[18px] text-[var(--color-brand)] shrink-0"
                  style={{ fontVariationSettings: "'FILL' 1" }}
                >
                  check_circle
                </span>
              )}
            </div>
          )
        })}
        {modes.length === 0 && (
          <div className="text-xs text-[var(--color-text-tertiary)] py-4 text-center">
            {t('settings.codingMode.loading')}
          </div>
        )}
      </div>

      <DebugPrivacySettings />
    </div>
  )
}

function DebugPrivacySettings() {
  const t = useTranslation()
  const piiSanitizer = useSettingsStore((s) => s.piiSanitizer)
  const setPiiEnabled = useSettingsStore((s) => s.setPiiEnabled)
  const setPiiKindEnabled = useSettingsStore((s) => s.setPiiKindEnabled)
  const resetPiiSanitizer = useSettingsStore((s) => s.resetPiiSanitizer)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionStats = useChatStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.debugPiiStats : undefined,
  )
  const resetDebugPiiStats = useChatStore((s) => s.resetDebugPiiStats)

  const disabledSet = useMemo(
    () => new Set<PiiKindLabel>(piiSanitizer.disabledKinds),
    [piiSanitizer.disabledKinds],
  )

  return (
    <div className="mt-6 border-t border-[var(--color-border)] pt-4">
      <h3 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">
        {t('settings.debugPrivacy.title')}
      </h3>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-4">
        {t('settings.debugPrivacy.description')}
      </p>

      <label className="flex items-center justify-between gap-3 mb-3 px-3 py-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
        <div className="flex flex-col">
          <span className="text-xs font-medium text-[var(--color-text-primary)]">
            {t('settings.debugPrivacy.enable')}
          </span>
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.debugPrivacy.enableHint')}
          </span>
        </div>
        <input
          type="checkbox"
          checked={piiSanitizer.enabled}
          onChange={(e) => setPiiEnabled(e.target.checked)}
          className="h-4 w-4"
        />
      </label>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 mb-3">
        {PII_KIND_LABELS.map((kind) => {
          const enabled = !disabledSet.has(kind)
          return (
            <label
              key={kind}
              className={`flex items-center justify-between gap-2 px-2.5 py-1.5 rounded border text-xs ${
                piiSanitizer.enabled
                  ? 'border-[var(--color-border)] bg-[var(--color-surface)]'
                  : 'border-[var(--color-border)] bg-[var(--color-surface-container-low)] opacity-60'
              }`}
            >
              <span className="text-[var(--color-text-secondary)]">
                {t(`debug.privacy.categories.${kind}` as TranslationKey)}
              </span>
              <input
                type="checkbox"
                disabled={!piiSanitizer.enabled}
                checked={enabled}
                onChange={(e) => setPiiKindEnabled(kind, e.target.checked)}
                className="h-3.5 w-3.5"
              />
            </label>
          )
        })}
      </div>

      <div className="flex items-center justify-between gap-2 mb-3 px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] text-xs">
        <span className="text-[var(--color-text-secondary)]">
          {t('settings.debugPrivacy.sessionStats')}
        </span>
        <span className="font-mono text-[var(--color-text-primary)]">
          {sessionStats?.total ?? 0}
        </span>
      </div>

      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => activeTabId && resetDebugPiiStats(activeTabId)}
          disabled={!activeTabId || (sessionStats?.total ?? 0) === 0}
        >
          {t('settings.debugPrivacy.clearStats')}
        </Button>
        <Button variant="ghost" size="sm" onClick={resetPiiSanitizer}>
          {t('settings.debugPrivacy.resetDefaults')}
        </Button>
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
  const closeBehavior = useSettingsStore((s) => s.closeBehavior)
  const setCloseBehavior = useSettingsStore((s) => s.setCloseBehavior)
  const autonomyData = useAutonomyStore((s) => s.data)
  const autonomyFetch = useAutonomyStore((s) => s.fetch)
  const autonomyUpdate = useAutonomyStore((s) => s.updatePartial)
  const autonomyHasFetched = useAutonomyStore((s) => s.hasFetched)
  const autonomyIsLoading = useAutonomyStore((s) => s.isLoading)
  const autonomyIsSaving = useAutonomyStore((s) => s.isSaving)
  const t = useTranslation()

  useEffect(() => {
    if (!autonomyHasFetched && !autonomyIsLoading) {
      void autonomyFetch()
    }
  }, [autonomyHasFetched, autonomyIsLoading, autonomyFetch])

  const enableCommandPolicy = autonomyData?.enableCommandPolicy ?? false

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

  const CLOSE_BEHAVIORS: Array<{ value: CloseBehavior; label: string }> = [
    { value: 'minimize', label: t('settings.general.closeBehavior.minimize') },
    { value: 'exit', label: t('settings.general.closeBehavior.exit') },
    { value: 'ask', label: t('settings.general.closeBehavior.ask') },
  ]

  return (
    <div>
      {}
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.appearanceTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.appearanceDescription')}</p>
      <div className="flex flex-wrap gap-2 mb-4">
        {THEMES.map(({ value, label }) => (
          <button
            key={value}
            onClick={() => void setTheme(value)}
            className={`h-7 px-4 min-w-[88px] text-xs font-semibold rounded-lg border transition-all ${
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
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.closeBehaviorTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.closeBehaviorDescription')}</p>
      <div className="flex flex-wrap gap-2 mb-4">
        {CLOSE_BEHAVIORS.map(({ value, label }) => (
          <button
            key={value}
            onClick={() => void setCloseBehavior(value)}
            className={`h-7 px-4 min-w-[88px] text-xs font-semibold rounded-lg border transition-all ${
              closeBehavior === value
                ? 'bg-[var(--color-brand)] text-white border-[var(--color-brand)]'
                : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      {}
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.languageTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.languageDescription')}</p>
      <div className="flex flex-wrap gap-2 mb-4">
        {LANGUAGES.map(({ value, label }) => (
          <button
            key={value}
            onClick={() => setLocale(value)}
            className={`h-7 px-4 min-w-[88px] text-xs font-semibold rounded-lg border transition-all ${
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
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.effortTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.effortDescription')}</p>
      <div className="flex flex-wrap gap-2 mb-4">
        {(['low', 'medium', 'high', 'max'] as EffortLevel[]).map((level) => (
          <button
            key={level}
            onClick={() => setEffort(level)}
            className={`h-7 px-4 min-w-[72px] text-xs font-semibold rounded-lg border transition-all ${
              effortLevel === level
                ? 'bg-[var(--color-brand)] text-white border-[var(--color-brand)]'
                : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
            }`}
          >
            {EFFORT_LABELS[level]}
          </button>
        ))}
      </div>

      {}
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">{t('settings.general.securityPolicyTitle')}</h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">{t('settings.general.securityPolicyDescription')}</p>
      <div className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] px-3 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium text-[var(--color-text-primary)]">
            {t('settings.general.securityPolicyToggle')}
          </div>
          <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
            {enableCommandPolicy
              ? t('settings.general.securityPolicyEnabledHint')
              : t('settings.general.securityPolicyDisabledHint')}
          </div>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={enableCommandPolicy}
          disabled={autonomyIsSaving || autonomyIsLoading}
          onClick={() => {
            void autonomyUpdate({ enableCommandPolicy: !enableCommandPolicy })
          }}
          className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors disabled:opacity-50 ${
            enableCommandPolicy ? 'bg-[var(--color-brand)]' : 'bg-[var(--color-surface-hover)]'
          }`}
        >
          <span
            className={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${
              enableCommandPolicy ? 'translate-x-5' : 'translate-x-0.5'
            }`}
          />
        </button>
      </div>

      <LanUserGroupSection />

      <SettingsSyncSection />

      <NetworkProxySection />

      <AutomationSection />

      <SecuritySandboxSection />

    </div>
  )
}

function LanUserGroupSection() {
  const t = useTranslation()
  const identity = useLanStore((s) => s.identity)
  const init = useLanStore((s) => s.init)
  const updateProfile = useLanStore((s) => s.updateProfile)
  const setDiscovery = useLanStore((s) => s.setDiscovery)
  const [nickname, setNickname] = useState('')
  const [email, setEmail] = useState('')
  const [saving, setSaving] = useState(false)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    void init()
  }, [init])

  useEffect(() => {
    if (identity) {
      setNickname(identity.nickname ?? '')
      setEmail(identity.email ?? '')
    }
  }, [identity?.nickname, identity?.email])

  const running = identity?.running ?? false
  const dirty =
    !!identity &&
    (nickname.trim() !== (identity.nickname ?? '') ||
      email.trim() !== (identity.email ?? ''))

  async function save() {
    setSaving(true)
    try {
      await updateProfile({
        nickname: nickname.trim() || undefined,
        email: email.trim() ? email.trim() : null,
      })
    } finally {
      setSaving(false)
    }
  }

  async function toggleDiscovery() {
    setBusy(true)
    try {
      await setDiscovery(!running)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mt-6">
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">
        {t('settings.userGroup.title')}
      </h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-3">
        {t('settings.userGroup.description')}
      </p>

      <div className="space-y-3">
        <div>
          <label className="text-xs font-medium text-[var(--color-text-secondary)] mb-1 block">
            {t('settings.userGroup.nickname')}
          </label>
          <Input value={nickname} onChange={(e) => setNickname(e.target.value)} />
        </div>

        <div>
          <label className="text-xs font-medium text-[var(--color-text-secondary)] mb-1 block">
            {t('settings.userGroup.email')}
          </label>
          <Input
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="name@example.com"
          />
        </div>

        <div className="flex justify-end">
          <Button onClick={() => void save()} disabled={!dirty || saving}>
            {saving ? t('common.saving') : t('common.save')}
          </Button>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="rounded-lg border border-[var(--color-border)] px-3 py-2">
            <div className="text-xs text-[var(--color-text-tertiary)]">
              {t('settings.userGroup.userId')}
            </div>
            <div className="text-xs font-mono text-[var(--color-text-primary)] mt-0.5 break-all">
              {identity?.userId ?? '—'}
            </div>
          </div>
          <div className="rounded-lg border border-[var(--color-border)] px-3 py-2">
            <div className="text-xs text-[var(--color-text-tertiary)]">
              {t('settings.userGroup.localIp')}
            </div>
            <div className="text-xs font-mono text-[var(--color-text-primary)] mt-0.5 break-all">
              {identity?.localIp ?? '—'}
            </div>
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] px-3 py-2.5">
          <div className="min-w-0 flex-1">
            <div className="text-xs font-medium text-[var(--color-text-primary)]">
              {t('settings.userGroup.discoveryToggle')}
            </div>
            <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
              {running
                ? t('settings.userGroup.discoveryEnabledHint')
                : t('settings.userGroup.discoveryDisabledHint')}
            </div>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={running}
            disabled={busy}
            onClick={() => void toggleDiscovery()}
            className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors disabled:opacity-50 ${
              running ? 'bg-[var(--color-brand)]' : 'bg-[var(--color-surface-hover)]'
            }`}
          >
            <span
              className={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${
                running ? 'translate-x-5' : 'translate-x-0.5'
              }`}
            />
          </button>
        </div>
      </div>
    </div>
  )
}
