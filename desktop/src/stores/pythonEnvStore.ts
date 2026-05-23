// SPDX-License-Identifier: MIT

import { create } from 'zustand'
import {
  pythonApi,
  type DiscoverResult,
  type PythonEnvEvent,
  type PythonEnvStatus,
} from '../api/python'
import { useUIStore } from './uiStore'
import { t } from '../i18n'

export type PythonJobKind = 'creating' | 'installing'

export type PythonJobState = {
  kind: PythonJobKind
  startedAt: number
  lines: string[]
  tool?: string
}

const MAX_JOB_LINES = 200

type StatusByRoot = Record<string, PythonEnvStatus | undefined>
type DiscoveryByRoot = Record<string, DiscoverResult | undefined>
type JobsByRoot = Record<string, PythonJobState | undefined>
type ErrorByRoot = Record<string, string | undefined>
type LoadingByRoot = Record<string, boolean | undefined>
type StreamByRoot = Record<string, EventSource | undefined>

type PythonEnvStore = {
  statusByRoot: StatusByRoot
  discoveryByRoot: DiscoveryByRoot
  jobsByRoot: JobsByRoot
  errorByRoot: ErrorByRoot
  loadingByRoot: LoadingByRoot
  streams: StreamByRoot
  activeRoot: string | null

  refresh: (root: string) => Promise<PythonEnvStatus | null>
  discover: (root: string) => Promise<DiscoverResult | null>
  createVenv: (root: string, tool?: 'uv' | 'venv', pythonVersion?: string) => Promise<void>
  selectInterpreter: (root: string, path: string) => Promise<PythonEnvStatus | null>
  installRequirements: (root: string, file?: string) => Promise<void>
  installSmart: (root: string) => Promise<void>
  purge: (root: string) => Promise<void>
  subscribe: (root: string) => void
  unsubscribe: (root: string) => void
  unsubscribeAll: () => void
  setActiveRoot: (root: string | null) => void
}

function clearJob(state: PythonEnvStore, root: string): JobsByRoot {
  const { [root]: _ignored, ...rest } = state.jobsByRoot
  return rest
}

function appendJobLine(state: PythonEnvStore, root: string, line: string): JobsByRoot {
  const prev = state.jobsByRoot[root]
  if (!prev) return state.jobsByRoot
  const lines = [...prev.lines, line].slice(-MAX_JOB_LINES)
  return {
    ...state.jobsByRoot,
    [root]: { ...prev, lines },
  }
}

function pushToast(
  type: 'success' | 'error' | 'warning' | 'info',
  message: string,
  action?: { label: string; onClick: () => void },
  duration?: number,
) {
  try {
    useUIStore.getState().addToast({
      type,
      message,
      duration: duration ?? (type === 'error' ? 12_000 : 6_000),
      action,
    })
  } catch (err) {
    console.warn('[pythonEnv] toast failed', err)
  }
}

export const usePythonEnvStore = create<PythonEnvStore>((set, get) => ({
  statusByRoot: {},
  discoveryByRoot: {},
  jobsByRoot: {},
  errorByRoot: {},
  loadingByRoot: {},
  streams: {},
  activeRoot: null,

  refresh: async (root) => {
    if (!root) return null
    set((s) => ({ loadingByRoot: { ...s.loadingByRoot, [root]: true } }))
    try {
      const status = await pythonApi.status(root)
      set((s) => ({
        statusByRoot: { ...s.statusByRoot, [root]: status },
        loadingByRoot: { ...s.loadingByRoot, [root]: false },
        errorByRoot: { ...s.errorByRoot, [root]: undefined },
      }))
      return status
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set((s) => ({
        loadingByRoot: { ...s.loadingByRoot, [root]: false },
        errorByRoot: { ...s.errorByRoot, [root]: message },
      }))
      return null
    }
  },

  discover: async (root) => {
    if (!root) return null
    try {
      const result = await pythonApi.discover(root)
      set((s) => ({ discoveryByRoot: { ...s.discoveryByRoot, [root]: result } }))
      return result
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set((s) => ({ errorByRoot: { ...s.errorByRoot, [root]: message } }))
      return null
    }
  },

  createVenv: async (root, tool, pythonVersion) => {
    if (!root) return
    set((s) => ({
      jobsByRoot: {
        ...s.jobsByRoot,
        [root]: {
          kind: 'creating',
          startedAt: Date.now(),
          lines: [],
          tool,
        },
      },
      errorByRoot: { ...s.errorByRoot, [root]: undefined },
    }))
    get().subscribe(root)
    pushToast('info', t('python.toast.creationStarted'))
    try {
      await pythonApi.create({ workspace: root, tool, pythonVersion })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set((s) => ({
        jobsByRoot: clearJob(s, root),
        errorByRoot: { ...s.errorByRoot, [root]: message },
      }))
      pushToast('error', t('python.toast.creationFailed', { error: message }))
    }
  },

  selectInterpreter: async (root, path) => {
    if (!root) return null
    try {
      const status = await pythonApi.select({ workspace: root, interpreterPath: path })
      set((s) => ({
        statusByRoot: { ...s.statusByRoot, [root]: status },
        errorByRoot: { ...s.errorByRoot, [root]: undefined },
      }))
      pushToast('success', t('python.toast.interpreterSelected'))
      return status
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set((s) => ({ errorByRoot: { ...s.errorByRoot, [root]: message } }))
      pushToast('error', message)
      return null
    }
  },

  installRequirements: async (root, file) => {
    if (!root) return
    set((s) => ({
      jobsByRoot: {
        ...s.jobsByRoot,
        [root]: { kind: 'installing', startedAt: Date.now(), lines: [] },
      },
    }))
    get().subscribe(root)
    try {
      await pythonApi.installRequirements({ workspace: root, file })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set((s) => ({
        jobsByRoot: clearJob(s, root),
        errorByRoot: { ...s.errorByRoot, [root]: message },
      }))
      pushToast('error', message)
    }
  },

  installSmart: async (root) => {
    if (!root) return
    set((s) => ({
      jobsByRoot: {
        ...s.jobsByRoot,
        [root]: { kind: 'installing', startedAt: Date.now(), lines: [] },
      },
    }))
    get().subscribe(root)
    try {
      await pythonApi.installSmart(root)
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set((s) => ({
        jobsByRoot: clearJob(s, root),
        errorByRoot: { ...s.errorByRoot, [root]: message },
      }))
      pushToast('error', message)
    }
  },

  purge: async (root) => {
    if (!root) return
    try {
      await pythonApi.purge(root)
      set((s) => ({
        statusByRoot: { ...s.statusByRoot, [root]: undefined },
        jobsByRoot: clearJob(s, root),
        errorByRoot: { ...s.errorByRoot, [root]: undefined },
      }))
      pushToast('success', t('python.toast.purgeDone'))
      void get().refresh(root)
      void get().discover(root)
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      pushToast('error', message)
    }
  },

  subscribe: (root) => {
    if (!root) return
    const current = get().streams[root]
    if (current) return
    const source = pythonApi.streamEvents(root, (event: PythonEnvEvent) => {
      switch (event.kind) {
        case 'snapshot':
          set((s) => ({
            statusByRoot: { ...s.statusByRoot, [root]: event.state },
          }))
          return
        case 'creating':
          set((s) => ({
            jobsByRoot: {
              ...s.jobsByRoot,
              [root]: {
                kind: 'creating',
                startedAt: Date.now(),
                lines: [],
                tool: event.tool,
              },
            },
          }))
          return
        case 'progress':
          set((s) => ({ jobsByRoot: appendJobLine(s, root, event.message) }))
          return
        case 'ready':
          set((s) => ({ jobsByRoot: clearJob(s, root) }))
          if (event.fallbackUsed) {
            pushToast('warning', t('python.toast.fallbackVenv'))
          } else {
            pushToast('success', t('python.toast.creationDone'))
          }
          void get().refresh(root)
          return
        case 'failed':
          set((s) => ({
            jobsByRoot: clearJob(s, root),
            errorByRoot: { ...s.errorByRoot, [root]: event.error },
          }))
          pushToast('error', t('python.toast.creationFailed', { error: event.error }))
          void get().refresh(root)
          return
        case 'install_start':
          set((s) => ({
            jobsByRoot: {
              ...s.jobsByRoot,
              [root]: { kind: 'installing', startedAt: Date.now(), lines: [] },
            },
          }))
          return
        case 'install_progress':
          set((s) => ({ jobsByRoot: appendJobLine(s, root, event.line) }))
          return
        case 'install_done':
          set((s) => ({
            jobsByRoot: clearJob(s, root),
            errorByRoot: event.success
              ? { ...s.errorByRoot, [root]: undefined }
              : { ...s.errorByRoot, [root]: event.message ?? 'install failed' },
          }))
          if (event.success) {
            pushToast('success', t('python.toast.installDone'))
          } else {
            pushToast('error', t('python.toast.installFailed', { error: event.message ?? '' }))
          }
          void get().refresh(root)
          return
        case 'packages_counted':
          set((s) => {
            const current = s.statusByRoot[root]
            if (!current) return s
            return {
              statusByRoot: {
                ...s.statusByRoot,
                [root]: { ...current, packagesCount: event.count },
              },
            }
          })
          return
        case 'purged':
          set((s) => ({
            statusByRoot: { ...s.statusByRoot, [root]: undefined },
            jobsByRoot: clearJob(s, root),
          }))
      }
    })
    set((s) => ({ streams: { ...s.streams, [root]: source } }))
  },

  unsubscribe: (root) => {
    const current = get().streams[root]
    if (current) {
      current.close()
      set((s) => {
        const { [root]: _ignored, ...rest } = s.streams
        return { streams: rest }
      })
    }
  },

  unsubscribeAll: () => {
    const { streams } = get()
    Object.values(streams).forEach((source) => {
      if (source) source.close()
    })
    set({ streams: {} })
  },

  setActiveRoot: (root) => {
    const prev = get().activeRoot
    if (prev === root) return
    if (prev) {
      get().unsubscribe(prev)
    }
    set({ activeRoot: root })
    if (root) {
      get().subscribe(root)
    }
  },
}))
