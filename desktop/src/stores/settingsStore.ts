import { create } from 'zustand'
import { settingsApi } from '../api/settings'
import { modelsApi } from '../api/models'
import { codingModesApi } from '../api/codingModes'
import type { PermissionMode, EffortLevel, ModelInfo, ThemeMode } from '../types/settings'
import type { CodingModeId, CodingModeInfo } from '../types/codingMode'
import { DEFAULT_CODING_MODE } from '../types/codingMode'
import type { Locale } from '../i18n'
import { useUIStore } from './uiStore'
import { useAutonomyStore } from './autonomyStore'

type PendingCodingModeTransition = {
  target: CodingModeId
  resolver: (confirmed: boolean) => void
}

const LOCALE_STORAGE_KEY = 'sen-locale'

function getStoredLocale(): Locale {
  try {
    const stored = localStorage.getItem(LOCALE_STORAGE_KEY)
    if (stored === 'en' || stored === 'zh') return stored
  } catch {  }
  return 'zh'
}

type SettingsStore = {

  codingMode: CodingModeId

  codingModes: CodingModeInfo[]

  permissionMode: PermissionMode
  currentModel: ModelInfo | null
  effortLevel: EffortLevel
  availableModels: ModelInfo[]
  activeProviderName: string | null
  locale: Locale
  theme: ThemeMode
  isLoading: boolean
  error: string | null

  pendingCodingModeTransition: PendingCodingModeTransition | null

  fetchAll: () => Promise<void>
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
}

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  codingMode: DEFAULT_CODING_MODE,
  codingModes: [],
  permissionMode: 'default',
  currentModel: null,
  effortLevel: 'medium',
  availableModels: [],
  activeProviderName: null,
  locale: getStoredLocale(),
  theme: useUIStore.getState().theme,
  isLoading: false,
  error: null,
  pendingCodingModeTransition: null,

  fetchAll: async () => {
    set({ isLoading: true, error: null })
    try {
      const [
        { mode: legacyPermMode },
        modelsRes,
        { model },
        { level },
        userSettings,
        codingCatalog,
        codingCurrent,
      ] = await Promise.all([
        settingsApi.getPermissionMode(),
        modelsApi.list(),
        modelsApi.getCurrent(),
        modelsApi.getEffort(),
        settingsApi.getUser(),
        codingModesApi.list(),
        codingModesApi.getCurrent(),
      ])
      const theme = userSettings.theme === 'dark' ? 'dark' : 'light'
      useUIStore.getState().setTheme(theme)
      set({
        codingMode: codingCurrent.mode,
        codingModes: codingCatalog.modes,
        permissionMode: legacyPermMode,
        availableModels: modelsRes.models,
        activeProviderName: modelsRes.provider?.name ?? null,
        currentModel: model,
        effortLevel: level,
        theme,
        isLoading: false,
        error: null,
      })
    } catch (error) {
      const message =
        error instanceof Error ? error.message : 'Failed to load desktop settings'
      set({ isLoading: false, error: message })
      throw error
    }
  },

  setCodingMode: async (mode) => {
    const prev = get().codingMode
    set({ codingMode: mode })
    try {
      const res = await codingModesApi.setCurrent(mode)

      const derived = (res.permissionMode as PermissionMode) || get().permissionMode
      set({ permissionMode: derived })
    } catch {
      set({ codingMode: prev })
    }
  },

  requestSetCodingMode: async (mode) => {
    if (get().codingMode === mode) return

    const autonomy = useAutonomyStore.getState().data
    const whitelist = autonomy?.autoApproveModeTransitions ?? []
    if (whitelist.includes(mode)) {
      await get().setCodingMode(mode)
      return
    }

    const existing = get().pendingCodingModeTransition
    if (existing) {
      existing.resolver(false)
    }

    await new Promise<void>((resolve, reject) => {
      set({
        pendingCodingModeTransition: {
          target: mode,
          resolver: (confirmed) => {
            if (confirmed) {
              get()
                .setCodingMode(mode)
                .then(resolve)
                .catch(reject)
            } else {
              resolve()
            }
          },
        },
      })
    })
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
    set({ codingMode: mode, permissionMode: derivedPermission })
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
    set({ locale })
    try { localStorage.setItem(LOCALE_STORAGE_KEY, locale) } catch {  }
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
}))
