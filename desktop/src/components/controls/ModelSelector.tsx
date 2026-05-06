import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useChatStore } from '../../stores/chatStore'
import { useProviderStore } from '../../stores/providerStore'
import { DRAFT_RUNTIME_SELECTION_KEY, useSessionRuntimeStore } from '../../stores/sessionRuntimeStore'
import { useSettingsStore } from '../../stores/settingsStore'
import type { SavedProvider } from '../../types/provider'
import type { RuntimeSelection } from '../../types/runtime'
import type { EffortLevel, ModelInfo } from '../../types/settings'
import { isValidRuntimeSelection } from '../../utils/runtimeSelection'

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
}

function buildProviderModels(provider: SavedProvider): ModelInfo[] {
  const seen = new Set<string>()
  const out: ModelInfo[] = []
  for (const raw of provider.models ?? []) {
    const id = raw.trim()
    if (!id || seen.has(id)) continue
    seen.add(id)
    out.push({ id, name: id, description: '', context: '' })
  }
  return out
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
  activeId: string | null,
  activeProviderName: string | null,
  providers: SavedProvider[],
  currentModelId: string | undefined,
): RuntimeSelection | null {
  const inferredProviderId = activeId ?? (
    activeProviderName
      ? providers.find((provider) => provider.name === activeProviderName)?.id ?? null
      : null
  )

  if (!inferredProviderId) {
    return null
  }

  const activeProvider = providers.find((provider) => provider.id === inferredProviderId)
  const activeModels = (activeProvider?.models ?? [])
    .map((id) => id.trim())
    .filter((id) => id.length > 0)

  const trimmedCurrent = currentModelId?.trim()
  const modelId = trimmedCurrent && activeModels.includes(trimmedCurrent)
    ? trimmedCurrent
    : activeModels[0]

  if (!modelId) {
    return null
  }

  return {
    providerId: inferredProviderId,
    modelId,
  }
}

export function ModelSelector({
  value,
  onChange,
  runtimeKey,
  disabled = false,
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
  const requestedProvidersRef = useRef(false)

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

  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
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

  const providerChoices = useMemo(
    () => buildProviderChoices(providers, activeId),
    [activeId, providers],
  )

  const selectedModel = isControlled
    ? availableModels.find((model) => model.id === value) || null
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

  const activeRuntimeSelection = isRuntimeScoped
    ? validRuntimeSelection ?? resolveDefaultRuntimeSelection(
      activeId,
      activeProviderName,
      providers,
      storeModel?.id,
    )
    : null

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

  const buttonModelLabel = isRuntimeScoped
    ? selectedRuntimeModel?.name ?? t('model.selectModel')
    : selectedModel?.name ?? t('model.selectModel')
  const buttonProviderLabel = isRuntimeScoped
    ? selectedProviderChoice?.providerName ?? null
    : null

  const handleRuntimeSelect = (selection: RuntimeSelection) => {
    if (!runtimeKey) return
    useSessionRuntimeStore.getState().setSelection(runtimeKey, selection)

    if (runtimeKey !== DRAFT_RUNTIME_SELECTION_KEY) {
      useSessionRuntimeStore.getState().setSelection(DRAFT_RUNTIME_SELECTION_KEY, selection)
      useChatStore.getState().setSessionRuntime(runtimeKey, selection)
    }
    setOpen(false)
  }

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => !disabled && setOpen(!open)}
        disabled={disabled}
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

      {open && (
        <div className="absolute right-0 bottom-full z-50 mb-2 w-[220px] rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]">
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
            ) : availableModels.length === 0 ? (
              <div className="rounded-lg border border-dashed border-[var(--color-border)] px-3 py-5 text-center text-xs text-[var(--color-text-tertiary)]">
                {t('settings.providers.empty')}
              </div>
            ) : (
              <div className="space-y-0.5">
                {availableModels.map((model) => {
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
        </div>
      )}
    </div>
  )
}
