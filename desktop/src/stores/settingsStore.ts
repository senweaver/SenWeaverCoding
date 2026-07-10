// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { settingsApi } from '../api/settings'
import { modelsApi } from '../api/models'
import { codingModesApi } from '../api/codingModes'
import { ApiError } from '../api/client'
import { wsManager } from '../api/websocket'
import type { PermissionMode, EffortLevel, ModelInfo, ThemeMode, CloseBehavior } from '../types/settings'
import type { CodingModeId, CodingModeInfo } from '../types/codingMode'
import {
  DEFAULT_CODING_MODE,
  isVisibleCodingMode,
  VISIBLE_CODING_MODES,
} from '../types/codingMode'
import type { Locale } from '../i18n'
import { useUIStore } from './uiStore'
type PendingCodingModeTransition = {
  target: CodingModeId
  resolver: (confirmed: boolean) => void
}

const LOCALE_STORAGE_KEY = 'sen-locale'

export async function syncTrayLabels(locale: Locale): Promise<void> {
  if (typeof window === 'undefined') return
  if (!('__TAURI_INTERNALS__' in window || '__TAURI__' in window)) return
  try {
    const [{ invoke }, { translate }] = await Promise.all([
      import('@tauri-apps/api/core'),
      import('../i18n'),
    ])
    await invoke('set_tray_labels', {
      show: translate(locale, 'tray.show'),
      stopComputer: translate(locale, 'tray.stopComputerControl'),
      quit: translate(locale, 'tray.quit'),
    })
  } catch {
    // Tray label sync is best-effort (e.g. tray not yet initialized); ignore.
  }
}

function getStoredLocale(): Locale {
  try {
    const stored = localStorage.getItem(LOCALE_STORAGE_KEY)
    if (stored === 'en' || stored === 'zh') return stored
  } catch {  }
  return 'zh'
}

const CLOSE_BEHAVIOR_STORAGE_KEY = 'sen-close-behavior'

function normalizeCloseBehavior(raw: unknown): CloseBehavior | null {
  return raw === 'minimize' || raw === 'exit' || raw === 'ask' ? raw : null
}

export function getStoredCloseBehavior(): CloseBehavior {
  try {
    const stored = normalizeCloseBehavior(localStorage.getItem(CLOSE_BEHAVIOR_STORAGE_KEY))
    if (stored) return stored
  } catch {  }
  return 'ask'
}

function storeCloseBehavior(value: CloseBehavior): void {
  try {
    localStorage.setItem(CLOSE_BEHAVIOR_STORAGE_KEY, value)
  } catch {  }
}

export const PII_KIND_LABELS = [
  'id_card',
  'phone',
  'email',
  'bank_card',
  'jwt',
  'api_key',
  'bearer',
  'auth_header',
  'url_password',
  'kv_secret',
  'private_key',
  'ipv4',
  'mac',
] as const

export type PiiKindLabel = (typeof PII_KIND_LABELS)[number]

export type PiiSanitizerSettings = {
  enabled: boolean
  disabledKinds: PiiKindLabel[]
}

const PII_STORAGE_KEY = 'sen-pii-sanitizer'

const DEFAULT_PII_SETTINGS: PiiSanitizerSettings = {
  enabled: true,
  disabledKinds: ['ipv4', 'mac'],
}

const PII_LEGACY_ALIASES: Record<string, PiiKindLabel> = {
  authorization_header: 'auth_header',
  mac_address: 'mac',
}

function normalizePiiLabel(raw: unknown): PiiKindLabel | null {
  if (typeof raw !== 'string') return null
  const lower = raw.toLowerCase()
  if (PII_KIND_LABELS.includes(lower as PiiKindLabel)) {
    return lower as PiiKindLabel
  }
  return PII_LEGACY_ALIASES[lower] ?? null
}

function getStoredPiiSettings(): PiiSanitizerSettings {
  try {
    const raw = localStorage.getItem(PII_STORAGE_KEY)
    if (!raw) return { ...DEFAULT_PII_SETTINGS }
    const parsed = JSON.parse(raw) as Partial<PiiSanitizerSettings>
    if (!parsed || typeof parsed !== 'object') return { ...DEFAULT_PII_SETTINGS }
    const enabled =
      typeof parsed.enabled === 'boolean' ? parsed.enabled : DEFAULT_PII_SETTINGS.enabled
    const disabledKinds = Array.isArray(parsed.disabledKinds)
      ? Array.from(
          new Set(
            parsed.disabledKinds
              .map(normalizePiiLabel)
              .filter((k): k is PiiKindLabel => k !== null),
          ),
        )
      : [...DEFAULT_PII_SETTINGS.disabledKinds]
    return { enabled, disabledKinds }
  } catch {
    return { ...DEFAULT_PII_SETTINGS }
  }
}

function storePiiSettings(settings: PiiSanitizerSettings): void {
  try {
    localStorage.setItem(PII_STORAGE_KEY, JSON.stringify(settings))
  } catch {
  }
}

const CODING_MODE_ORDER_STORAGE_KEY = 'sen-coding-mode-order'

function getStoredCodingModeOrder(): CodingModeId[] {
  try {
    const raw = localStorage.getItem(CODING_MODE_ORDER_STORAGE_KEY)
    if (!raw) return [...VISIBLE_CODING_MODES]
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return [...VISIBLE_CODING_MODES]
    const cleaned = parsed.filter(
      (id): id is CodingModeId => typeof id === 'string' && isVisibleCodingMode(id),
    )
    const merged = [...cleaned]
    for (const id of VISIBLE_CODING_MODES) {
      if (!merged.includes(id)) merged.push(id)
    }
    return merged
  } catch {
    return [...VISIBLE_CODING_MODES]
  }
}

function storeCodingModeOrder(order: CodingModeId[]): void {
  try {
    localStorage.setItem(CODING_MODE_ORDER_STORAGE_KEY, JSON.stringify(order))
  } catch {
  }
}

type SettingsStore = {

  codingMode: CodingModeId

  codingModes: CodingModeInfo[]

  codingModeOrder: CodingModeId[]

  permissionMode: PermissionMode
  currentModel: ModelInfo | null
  effortLevel: EffortLevel
  availableModels: ModelInfo[]
  activeProviderName: string | null
  locale: Locale
  theme: ThemeMode
  closeBehavior: CloseBehavior
  isLoading: boolean
  error: string | null

  piiSanitizer: PiiSanitizerSettings

  pendingCodingModeTransition: PendingCodingModeTransition | null

  fetchAll: () => Promise<void>
  fetchModels: () => Promise<void>
  setCodingModeOrder: (order: CodingModeId[]) => void
  setCodingMode: (mode: CodingModeId) => Promise<void>
  requestSetCodingMode: (mode: CodingModeId) => Promise<void>
  resolveCodingModeTransition: (confirmed: boolean) => void
  setPermissionMode: (mode: PermissionMode) => Promise<void>

  applyCodingMode: (mode: CodingModeId, derivedPermission: PermissionMode) => void

  applyPermissionMode: (mode: PermissionMode) => void
  setModel: (modelId: string) => Promise<void>
  setEffort: (level: EffortLevel) => Promise<void>
  setLocale: (locale: Locale) => void
  setTheme: (theme: ThemeMode) => Promise<void>
  setCloseBehavior: (behavior: CloseBehavior) => Promise<void>

  setPiiEnabled: (enabled: boolean) => void
  setPiiKindEnabled: (kind: PiiKindLabel, enabled: boolean) => void
  resetPiiSanitizer: () => void
}

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  codingMode: DEFAULT_CODING_MODE,
  codingModes: [],
  codingModeOrder: getStoredCodingModeOrder(),
  permissionMode: 'default',
  currentModel: null,
  effortLevel: 'medium',
  availableModels: [],
  activeProviderName: null,
  locale: getStoredLocale(),
  theme: useUIStore.getState().theme,
  closeBehavior: getStoredCloseBehavior(),
  isLoading: false,
  error: null,
  pendingCodingModeTransition: null,
  piiSanitizer: getStoredPiiSettings(),

  fetchModels: async () => {
    const settled = await Promise.allSettled([modelsApi.list(), modelsApi.getCurrent()])
    const [modelsListRes, currentModelRes] = settled
    const previous = get()
    const modelsRes =
      modelsListRes.status === 'fulfilled'
        ? modelsListRes.value
        : {
            models: previous.availableModels,
            provider: { name: previous.activeProviderName ?? '' },
          }
    const currentModel =
      currentModelRes.status === 'fulfilled'
        ? currentModelRes.value.model
        : previous.currentModel
    set({
      availableModels: modelsRes.models,
      activeProviderName: modelsRes.provider?.name ?? null,
      currentModel,
    })
  },

  fetchAll: async () => {
    set({ isLoading: true, error: null })

    const settled = await Promise.allSettled([
      settingsApi.getPermissionMode(),
      modelsApi.list(),
      modelsApi.getCurrent(),
      modelsApi.getEffort(),
      settingsApi.getUser(),
      codingModesApi.list(),
      codingModesApi.getCurrent(),
    ])

    const [
      permissionRes,
      modelsListRes,
      currentModelRes,
      effortRes,
      userSettingsRes,
      codingCatalogRes,
      codingCurrentRes,
    ] = settled

    const failures: string[] = []
    const noteFailure = (label: string, result: PromiseSettledResult<unknown>) => {
      if (result.status === 'rejected') {
        const reason = result.reason
        const detail =
          reason instanceof Error ? reason.message : String(reason ?? 'unknown')
        console.warn(`[settings] ${label} failed; using safe defaults:`, reason)
        failures.push(`${label}: ${detail}`)
      }
    }

    noteFailure('permission_mode', permissionRes)
    noteFailure('models_list', modelsListRes)
    noteFailure('current_model', currentModelRes)
    noteFailure('effort', effortRes)
    noteFailure('user_settings', userSettingsRes)
    noteFailure('coding_modes_catalog', codingCatalogRes)
    noteFailure('coding_modes_current', codingCurrentRes)

    const previous = get()
    const legacyPermMode =
      permissionRes.status === 'fulfilled' ? permissionRes.value.mode : previous.permissionMode
    const modelsRes =
      modelsListRes.status === 'fulfilled'
        ? modelsListRes.value
        : { models: previous.availableModels, provider: { name: previous.activeProviderName ?? '' } }
    const currentModel =
      currentModelRes.status === 'fulfilled' ? currentModelRes.value.model : previous.currentModel
    const effortLevel =
      effortRes.status === 'fulfilled' ? effortRes.value.level : previous.effortLevel
    const userSettings =
      userSettingsRes.status === 'fulfilled' ? userSettingsRes.value : { theme: previous.theme }
    const codingCatalog =
      codingCatalogRes.status === 'fulfilled'
        ? codingCatalogRes.value
        : { modes: previous.codingModes }
    const codingCurrent =
      codingCurrentRes.status === 'fulfilled'
        ? codingCurrentRes.value
        : { mode: previous.codingMode, permissionMode: legacyPermMode as string }

    const theme = userSettings.theme === 'dark' ? 'dark' : 'light'
    useUIStore.getState().setTheme(theme)
    const closeBehavior =
      normalizeCloseBehavior((userSettings as { closeBehavior?: unknown }).closeBehavior) ??
      previous.closeBehavior
    storeCloseBehavior(closeBehavior)
    const initialMode: CodingModeId = isVisibleCodingMode(codingCurrent.mode)
      ? codingCurrent.mode
      : DEFAULT_CODING_MODE
    set({
      codingMode: initialMode,
      codingModes: codingCatalog.modes,
      permissionMode: legacyPermMode,
      availableModels: modelsRes.models,
      activeProviderName: modelsRes.provider?.name ?? null,
      currentModel,
      effortLevel,
      theme,
      closeBehavior,
      isLoading: false,
      error: failures.length > 0 ? failures.join(' | ') : null,
    })
    if (
      codingCurrentRes.status === 'fulfilled'
      && initialMode !== codingCurrent.mode
    ) {
      codingModesApi
        .setCurrent(initialMode)
        .then((res) => {
          const derived =
            (res.permissionMode as PermissionMode) || get().permissionMode
          set({ permissionMode: derived })
        })
        .catch((err) => {
          console.warn('[settings] normalize hidden coding mode failed', err)
        })
    }
  },

  setCodingModeOrder: (order) => {
    const cleaned = order.filter(
      (id): id is CodingModeId => isVisibleCodingMode(id),
    )
    const merged = [...cleaned]
    for (const id of VISIBLE_CODING_MODES) {
      if (!merged.includes(id)) merged.push(id)
    }
    set({ codingModeOrder: merged })
    storeCodingModeOrder(merged)
  },

  setCodingMode: async (mode) => {
    if (!isVisibleCodingMode(mode)) {
      console.warn('[settings] setCodingMode rejected hidden mode', mode)
      return
    }
    const prev = get().codingMode
    set({ codingMode: mode })
    try {
      const res = await codingModesApi.setCurrent(mode, true)

      const derived = (res.permissionMode as PermissionMode) || get().permissionMode
      set({ permissionMode: derived })
    } catch {
      set({ codingMode: prev })
    }
  },

  requestSetCodingMode: async (mode) => {
    if (!isVisibleCodingMode(mode)) {
      console.warn('[settings] requestSetCodingMode rejected hidden mode', mode)
      return
    }
    if (get().codingMode === mode) return

    const existing = get().pendingCodingModeTransition
    if (existing) {
      existing.resolver(false)
    }
    set({ pendingCodingModeTransition: null })

    const prevMode = get().codingMode
    set({ codingMode: mode })

    try {
      const res = await codingModesApi.setCurrent(mode, false)
      const derived = (res.permissionMode as PermissionMode) || get().permissionMode
      set({ codingMode: mode, permissionMode: derived })
      return
    } catch (err) {
      const needsConfirm =
        err instanceof ApiError &&
        err.status === 409 &&
        !!err.body &&
        typeof err.body === 'object' &&
        (err.body as Record<string, unknown>).confirmationRequired === true
      set({ codingMode: prevMode })
      if (!needsConfirm) {
        console.warn('[settings] requestSetCodingMode failed', err)
        return
      }
    }

    const confirmed = await new Promise<boolean>((resolve) => {
      set({ pendingCodingModeTransition: { target: mode, resolver: resolve } })
    })
    if (!confirmed) return
    await get().setCodingMode(mode)
  },

  resolveCodingModeTransition: (confirmed) => {
    const pending = get().pendingCodingModeTransition
    if (!pending) return
    set({ pendingCodingModeTransition: null })
    pending.resolver(confirmed)
  },

  setPermissionMode: async (mode) => {
    const prev = get().permissionMode
    set({ permissionMode: mode })
    try {
      await settingsApi.setPermissionMode(mode)
    } catch {
      set({ permissionMode: prev })
    }
  },

  applyCodingMode: (mode, derivedPermission) => {
    const safeMode: CodingModeId = isVisibleCodingMode(mode) ? mode : DEFAULT_CODING_MODE
    set({ codingMode: safeMode, permissionMode: derivedPermission })
  },

  applyPermissionMode: (mode) => {
    set({ permissionMode: mode })
  },

  setModel: async (modelId) => {
    await modelsApi.setCurrent(modelId)
    const { model } = await modelsApi.getCurrent()
    set({ currentModel: model })
  },

  setEffort: async (level) => {
    const prev = get().effortLevel
    set({ effortLevel: level })
    try {
      await modelsApi.setEffort(level)
    } catch {
      set({ effortLevel: prev })
    }
  },

  setLocale: (locale) => {
    void import('../i18n')
      .then(({ ensureLocaleLoaded }) => ensureLocaleLoaded(locale))
      .catch(() => {})
      .then(() => {
        set({ locale })
        try { localStorage.setItem(LOCALE_STORAGE_KEY, locale) } catch {  }
        void syncTrayLabels(locale)
      })
  },

  setTheme: async (theme) => {
    const prev = get().theme
    set({ theme })
    useUIStore.getState().setTheme(theme)
    try {
      await settingsApi.updateUser({ theme })
    } catch {
      set({ theme: prev })
      useUIStore.getState().setTheme(prev)
    }
  },

  setCloseBehavior: async (behavior) => {
    const prev = get().closeBehavior
    set({ closeBehavior: behavior })
    storeCloseBehavior(behavior)
    try {
      await settingsApi.updateUser({ closeBehavior: behavior })
    } catch {
      set({ closeBehavior: prev })
      storeCloseBehavior(prev)
    }
  },

  setPiiEnabled: (enabled) => {
    const prev = get().piiSanitizer
    const next: PiiSanitizerSettings = { ...prev, enabled }
    set({ piiSanitizer: next })
    storePiiSettings(next)
    broadcastPiiConfig(next)
  },

  setPiiKindEnabled: (kind, enabled) => {
    const prev = get().piiSanitizer
    const set_ = new Set<PiiKindLabel>(prev.disabledKinds)
    if (enabled) set_.delete(kind)
    else set_.add(kind)
    const next: PiiSanitizerSettings = {
      enabled: prev.enabled,
      disabledKinds: PII_KIND_LABELS.filter((k) => set_.has(k)),
    }
    set({ piiSanitizer: next })
    storePiiSettings(next)
    broadcastPiiConfig(next)
  },

  resetPiiSanitizer: () => {
    const next: PiiSanitizerSettings = { ...DEFAULT_PII_SETTINGS }
    set({ piiSanitizer: next })
    storePiiSettings(next)
    broadcastPiiConfig(next)
  },
}))

function buildPiiMessage(settings: PiiSanitizerSettings) {
  return {
    type: 'set_pii_config' as const,
    data: {
      enabled: settings.enabled,
      disabledKinds: [...settings.disabledKinds],
    },
  }
}

function sendPiiConfigTo(sessionId: string, settings: PiiSanitizerSettings) {
  try {
    wsManager.send(sessionId, buildPiiMessage(settings))
  } catch (err) {
    console.warn('[settings] send pii config failed', err)
  }
}

function broadcastPiiConfig(settings: PiiSanitizerSettings): void {
  try {
    const sessionIds = wsManager.getConnectedSessionIds()
    for (const sid of sessionIds) {
      sendPiiConfigTo(sid, settings)
    }
  } catch (err) {
    console.warn('[settings] broadcast pii config failed', err)
  }
}

if (typeof window !== 'undefined') {
  wsManager.onConnected((sessionId) => {
    try {
      const settings = useSettingsStore.getState().piiSanitizer
      sendPiiConfigTo(sessionId, settings)
    } catch {
    }
  })
}
