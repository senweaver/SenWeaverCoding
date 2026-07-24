// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation, type TranslationKey } from '../../i18n'
import { useProviderStore } from '../../stores/providerStore'
import { DRAFT_RUNTIME_SELECTION_KEY, useSessionRuntimeStore } from '../../stores/sessionRuntimeStore'
import { useSettingsStore } from '../../stores/settingsStore'
import type { SavedProvider } from '../../types/provider'
import type { RuntimeSelection } from '../../types/runtime'
import type { EffortLevel, ModelInfo } from '../../types/settings'
import { isValidRuntimeSelection, persistRuntimeSelection, resolveEffectiveRuntimeSelection } from '../../utils/runtimeSelection'
import { syncRuntimeSelectionToBackend } from '../../utils/runtimeSync'
import { enabledProviderModelIds } from '../../utils/providerModels'
import { DEFAULT_MODEL_TYPE, buildModelTypeLookup, modelMatchesType, modelTypeLabelKey } from '../../utils/modelTypes'

type ProviderChoice = {
  providerId: string
  providerName: string
  isDefault: boolean
  models: ModelInfo[]
}

type Props = {
  value?: string
  onChange?: (modelId: string) => void
  runtimeKey?: string
  disabled?: boolean
  requiredType?: string
  modelPool?: ModelInfo[]
}

function buildProviderModels(provider: SavedProvider): ModelInfo[] {
  return enabledProviderModelIds(provider).map((id) => ({
    id,
    name: id,
    description: '',
    context: '',
  }))
}

function buildProviderChoices(
  providers: SavedProvider[],
  activeId: string | null,
): ProviderChoice[] {

  return providers
    .map((provider) => ({
      providerId: provider.id,
      providerName: provider.name,
      isDefault: activeId === provider.id,
      models: buildProviderModels(provider),
    }))
    .filter((choice) => choice.models.length > 0)
}

function resolveDefaultRuntimeSelection(
  runtimeKey: string,
  activeId: string | null,
  activeProviderName: string | null,
  providers: SavedProvider[],
  currentModelId: string | undefined,
): RuntimeSelection | null {
  return resolveEffectiveRuntimeSelection(
    runtimeKey,
    providers,
    activeId ?? (
      activeProviderName
        ? providers.find((provider) => provider.name === activeProviderName)?.id ?? null
        : null
    ),
    currentModelId,
  )
}

export function ModelSelector({
  value,
  onChange,
  runtimeKey,
  disabled = false,
  requiredType = DEFAULT_MODEL_TYPE,
  modelPool,
}: Props = {}) {
  const t = useTranslation()
  const storeModel = useSettingsStore((s) => s.currentModel)
  const availableModels = useSettingsStore((s) => s.availableModels)
  const effortLevel = useSettingsStore((s) => s.effortLevel)
  const activeProviderName = useSettingsStore((s) => s.activeProviderName)
  const setModel = useSettingsStore((s) => s.setModel)
  const setEffort = useSettingsStore((s) => s.setEffort)
  const providers = useProviderStore((s) => s.providers)
  const activeId = useProviderStore((s) => s.activeId)
  const providersLoading = useProviderStore((s) => s.isLoading)
  const fetchProviders = useProviderStore((s) => s.fetchProviders)
  const runtimeSelection = useSessionRuntimeStore((state) =>
    runtimeKey ? state.selections[runtimeKey] : undefined,
  )
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const [dropdownPos, setDropdownPos] = useState<{
    top: number
    left: number
    direction: 'up' | 'down'
  } | null>(null)
  const requestedProvidersRef = useRef(false)
  const lastAutoSyncedRef = useRef<string | null>(null)

  const EFFORT_OPTIONS: { value: EffortLevel; label: string }[] = [
    { value: 'low', label: t('settings.general.effort.low') },
    { value: 'medium', label: t('settings.general.effort.medium') },
    { value: 'high', label: t('settings.general.effort.high') },
    { value: 'max', label: t('settings.general.effort.max') },
  ]

  const isControlled = value !== undefined
  const isRuntimeScoped = !isControlled && runtimeKey !== undefined

  useEffect(() => {
    if (!isRuntimeScoped || providersLoading || requestedProvidersRef.current) return
    requestedProvidersRef.current = true
    void fetchProviders()
  }, [fetchProviders, isRuntimeScoped, providersLoading])

  const MENU_WIDTH = 220

  const updateDropdownPos = useCallback(() => {
    if (!triggerRef.current) return
    const rect = triggerRef.current.getBoundingClientRect()
    const DROPDOWN_HEIGHT = 460
    const spaceAbove = rect.top
    const spaceBelow = window.innerHeight - rect.bottom
    const direction = spaceBelow >= DROPDOWN_HEIGHT || spaceBelow >= spaceAbove ? 'down' : 'up'
    const left = Math.max(8, Math.min(rect.right - MENU_WIDTH, window.innerWidth - MENU_WIDTH - 8))
    setDropdownPos({
      top: direction === 'down' ? rect.bottom + 4 : rect.top - 4,
      left,
      direction,
    })
  }, [])

  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      const target = e.target as Node
      if (ref.current?.contains(target)) return
      if (menuRef.current?.contains(target)) return
      setOpen(false)
    }
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    document.addEventListener('keydown', handleEsc)
    return () => {
      document.removeEventListener('mousedown', handleClick)
      document.removeEventListener('keydown', handleEsc)
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    updateDropdownPos()
    window.addEventListener('scroll', updateDropdownPos, true)
    window.addEventListener('resize', updateDropdownPos)
    return () => {
      window.removeEventListener('scroll', updateDropdownPos, true)
      window.removeEventListener('resize', updateDropdownPos)
    }
  }, [open, updateDropdownPos])

  const typeLookup = useMemo(() => buildModelTypeLookup(providers), [providers])

  const renderTypeBadges = (modelId: string) => {
    const types = typeLookup.get(modelId) ?? [DEFAULT_MODEL_TYPE]
    return (
      <div className="mt-1 flex flex-wrap gap-1">
        {types.map((tp) => (
          <span
            key={tp}
            className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface-container-high)] px-1.5 py-[1px] text-[9px] font-medium leading-none text-[var(--color-text-tertiary)]"
          >
            {t(modelTypeLabelKey(tp) as TranslationKey)}
          </span>
        ))}
      </div>
    )
  }

  const providerChoices = useMemo(
    () =>
      buildProviderChoices(providers, activeId)
        .map((choice) => ({
          ...choice,
          models: choice.models.filter((model) =>
            modelMatchesType(typeLookup, model.id, requiredType),
          ),
        }))
        .filter((choice) => choice.models.length > 0),
    [activeId, providers, typeLookup, requiredType],
  )

  const baseModels = isControlled && modelPool ? modelPool : availableModels

  const filteredAvailableModels = useMemo(
    () =>
      baseModels.filter((model) =>
        modelMatchesType(typeLookup, model.id, requiredType),
      ),
    [baseModels, typeLookup, requiredType],
  )

  const selectedModel = isControlled
    ? baseModels.find((model) => model.id === value) || null
    : storeModel

  const validRuntimeSelection = useMemo(() => {
    if (!isRuntimeScoped) return null
    return isValidRuntimeSelection(runtimeSelection, providers) ? runtimeSelection : null
  }, [isRuntimeScoped, runtimeSelection, providers])

  useEffect(() => {
    if (!isRuntimeScoped || !runtimeKey) return
    if (providersLoading) return
    if (providers.length === 0) return
    if (!runtimeSelection) return
    if (!isValidRuntimeSelection(runtimeSelection, providers)) {
      useSessionRuntimeStore.getState().clearSelection(runtimeKey)
    }
  }, [isRuntimeScoped, runtimeKey, providersLoading, providers, runtimeSelection])

  const activeRuntimeSelection = isRuntimeScoped && runtimeKey
    ? validRuntimeSelection ?? resolveDefaultRuntimeSelection(
      runtimeKey,
      activeId,
      activeProviderName,
      providers,
      storeModel?.id,
    )
    : null

  useEffect(() => {
    if (!isRuntimeScoped || !runtimeKey || providersLoading || providers.length === 0) return
    if (runtimeKey !== DRAFT_RUNTIME_SELECTION_KEY) return
    if (!activeRuntimeSelection?.providerId || !activeRuntimeSelection.modelId.trim()) return
    if (validRuntimeSelection) {
      lastAutoSyncedRef.current = null
      return
    }
    const syncKey = `${runtimeKey}:${activeRuntimeSelection.providerId}:${activeRuntimeSelection.modelId}`
    if (lastAutoSyncedRef.current === syncKey) return
    lastAutoSyncedRef.current = syncKey
    persistRuntimeSelection(runtimeKey, activeRuntimeSelection)
    void syncRuntimeSelectionToBackend(activeRuntimeSelection, null, false)
  }, [
    isRuntimeScoped,
    runtimeKey,
    providersLoading,
    providers,
    activeRuntimeSelection,
    validRuntimeSelection,
  ])

  const selectedProviderChoice = activeRuntimeSelection
    ? providerChoices.find((choice) => choice.providerId === activeRuntimeSelection.providerId) ?? null
    : null

  const selectedRuntimeModel = activeRuntimeSelection
    ? selectedProviderChoice?.models.find((model) => model.id === activeRuntimeSelection.modelId)
      ?? {
        id: activeRuntimeSelection.modelId,
        name: activeRuntimeSelection.modelId,
        description: '',
        context: '',
      }
    : null

  const noConfiguredModels = isRuntimeScoped
    ? providerChoices.length === 0
    : filteredAvailableModels.length === 0
  const buttonModelLabel = noConfiguredModels
    ? t('model.unconfiguredPlaceholder')
    : isRuntimeScoped
      ? selectedRuntimeModel?.name ?? t('model.selectModel')
      : selectedModel?.name ?? t('model.selectModel')
  const buttonProviderLabel = noConfiguredModels
    ? null
    : isRuntimeScoped
      ? selectedProviderChoice?.providerName ?? null
      : null
  const buttonDisabled = disabled || noConfiguredModels

  const handleRuntimeSelect = (selection: RuntimeSelection) => {
    if (!runtimeKey) return
    persistRuntimeSelection(runtimeKey, selection)
    const scopedSessionId = runtimeKey !== DRAFT_RUNTIME_SELECTION_KEY ? runtimeKey : null
    void syncRuntimeSelectionToBackend(selection, scopedSessionId, scopedSessionId === null)
    setOpen(false)
  }

  return (
    <div ref={ref} className="relative">
      <button
        ref={triggerRef}
        onClick={() => !buttonDisabled && setOpen(!open)}
        disabled={buttonDisabled}
        title={noConfiguredModels ? t('model.unconfiguredPlaceholder') : undefined}
        className={`flex items-center gap-1.5 rounded-full bg-[var(--color-surface-container-low)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)] disabled:cursor-not-allowed disabled:opacity-50 ${
          isRuntimeScoped ? 'w-[220px] shrink-0' : 'max-w-[260px]'
        }`}
      >
        <div className="flex min-w-0 flex-1 items-center gap-1.5">
          <span className="min-w-0 flex-1 truncate text-[12px] font-semibold text-[var(--color-text-primary)]">
            {buttonModelLabel}
          </span>
          {buttonProviderLabel && (
            <span className="max-w-[100px] flex-shrink-0 truncate text-[10px] text-[var(--color-text-tertiary)]">
              {buttonProviderLabel}
            </span>
          )}
        </div>
        <span className="material-symbols-outlined flex-shrink-0 text-[11px]">expand_more</span>
      </button>

      {open && dropdownPos && createPortal(
        <div
          ref={menuRef}
          role="menu"
          className="w-[220px] rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]"
          style={{
            position: 'fixed',
            left: dropdownPos.left,
            ...(dropdownPos.direction === 'down'
              ? { top: dropdownPos.top }
              : { bottom: window.innerHeight - dropdownPos.top }),
            zIndex: 9999,
          }}
        >
          <div className="max-h-[420px] overflow-y-auto p-2">
            <div className="mb-1.5 px-1 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
              {t('model.configuration')}
            </div>

            {isRuntimeScoped ? (
              providerChoices.length === 0 ? (
                <div className="rounded-lg border border-dashed border-[var(--color-border)] px-3 py-5 text-center text-xs text-[var(--color-text-tertiary)]">
                  {t('settings.providers.empty')}
                </div>
              ) : (
              <div className="space-y-2.5">
                {providerChoices.map((choice) => (
                  <div key={choice.providerId} className="space-y-1">
                    <div className="flex items-center gap-1.5 px-1.5 pt-1">
                      <span
                        className="min-w-0 flex-1 truncate text-[11px] font-semibold tracking-[0.01em] text-[var(--color-text-secondary)]"
                        title={choice.providerName}
                      >
                        {choice.providerName}
                      </span>
                      {choice.isDefault && (
                        <span className="shrink-0 text-[10px] font-medium text-[var(--color-text-tertiary)]">
                          {t('settings.providers.default')}
                        </span>
                      )}
                    </div>

                    <div className="space-y-0.5">
                      {choice.models.map((model) => {
                        const isSelected =
                          activeRuntimeSelection?.providerId === choice.providerId &&
                          activeRuntimeSelection.modelId === model.id
                        return (
                          <button
                            key={`${choice.providerId}:${model.id}`}
                            onClick={() => handleRuntimeSelect({ providerId: choice.providerId, modelId: model.id })}
                            className={`
                              w-full rounded-lg border px-2.5 py-2 text-left transition-colors
                              ${isSelected
                                ? 'border-[var(--color-brand)]/20 bg-[var(--color-primary-fixed)]'
                                : 'border-transparent hover:bg-[var(--color-surface-hover)]'
                              }
                            `}
                          >
                            <div className="flex items-start gap-2">
                              <div className={`mt-0.5 flex h-4 w-4 flex-shrink-0 items-center justify-center rounded-full border-2 ${
                                isSelected ? 'border-[var(--color-brand)]' : 'border-[var(--color-outline)]'
                              }`}>
                                {isSelected && (
                                  <div className="h-2 w-2 rounded-full bg-[var(--color-brand)]" />
                                )}
                              </div>

                              <div className="min-w-0 flex-1">
                                <div
                                  className="truncate text-[13px] font-semibold text-[var(--color-text-primary)]"
                                  title={model.name}
                                >
                                  {model.name}
                                </div>
                                {model.description && (
                                  <div
                                    className="mt-0.5 truncate text-[10px] text-[var(--color-text-tertiary)]"
                                    title={model.description}
                                  >
                                    {model.description}
                                  </div>
                                )}
                                {renderTypeBadges(model.id)}
                              </div>
                            </div>
                          </button>
                        )
                      })}
                    </div>
                  </div>
                ))}
              </div>
              )
            ) : filteredAvailableModels.length === 0 ? (
              <div className="rounded-lg border border-dashed border-[var(--color-border)] px-3 py-5 text-center text-xs text-[var(--color-text-tertiary)]">
                {t('settings.providers.empty')}
              </div>
            ) : (
              <div className="space-y-0.5">
                {filteredAvailableModels.map((model) => {
                  const isSelected = model.id === selectedModel?.id
                  return (
                    <button
                      key={model.id}
                      onClick={() => {
                        if (isControlled) {
                          onChange?.(model.id)
                        } else {
                          void setModel(model.id)
                        }
                        setOpen(false)
                      }}
                      className={`
                        w-full rounded-lg px-2.5 py-2 text-left transition-colors
                        ${isSelected
                          ? 'bg-[var(--color-primary-fixed)] border border-[var(--color-brand)]/20'
                          : 'hover:bg-[var(--color-surface-hover)]'
                        }
                      `}
                    >
                      <div className="flex items-center gap-2">
                        <div className={`flex h-4 w-4 flex-shrink-0 items-center justify-center rounded-full border-2 ${
                          isSelected ? 'border-[var(--color-brand)]' : 'border-[var(--color-outline)]'
                        }`}>
                          {isSelected && (
                            <div className="h-2 w-2 rounded-full bg-[var(--color-brand)]" />
                          )}
                        </div>

                        <div className="min-w-0 flex-1">
                          <div
                            className="truncate text-[13px] font-semibold text-[var(--color-text-primary)]"
                            title={model.name}
                          >
                            {model.name}
                          </div>
                          {model.description && (
                            <div
                              className="mt-0.5 truncate text-[10px] text-[var(--color-text-tertiary)]"
                              title={model.description}
                            >
                              {model.description}
                            </div>
                          )}
                          {renderTypeBadges(model.id)}
                        </div>
                      </div>
                    </button>
                  )
                })}
              </div>
            )}
          </div>

          {!isControlled && !isRuntimeScoped && (
            <div className="border-t border-[var(--color-border)] p-2">
              <div className="mb-1.5 px-1 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
                {t('model.effort')}
              </div>
              <div className="grid grid-cols-4 gap-1">
                {EFFORT_OPTIONS.map((opt) => {
                  const isSelected = opt.value === effortLevel
                  return (
                    <button
                      key={opt.value}
                      onClick={() => {
                        void setEffort(opt.value)
                        setOpen(false)
                      }}
                      title={opt.label}
                      className={`
                        rounded-lg py-1.5 text-center text-[11px] font-semibold transition-colors
                        ${isSelected
                          ? 'bg-[var(--color-brand)] text-white'
                          : 'bg-[var(--color-surface-container-high)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                        }
                      `}
                    >
                      {opt.label}
                    </button>
                  )
                })}
              </div>
            </div>
          )}
        </div>,
        document.body,
      )}
    </div>
  )
}
