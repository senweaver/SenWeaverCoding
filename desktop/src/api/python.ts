// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api, getBaseUrl, withAuthToken } from './client'

export type PythonInterpreterTool = 'uv' | 'venv' | 'system' | 'unknown'

export type InstallStrategy =
  | 'uv_sync'
  | 'uv_pip_editable'
  | 'pip_editable'
  | 'uv_pip_requirements'
  | 'pip_requirements'
  | 'none'

export type InstallRecommendation = {
  strategy: InstallStrategy
  target: string | null
  uv_available: boolean
}

export type RequiredPython = {
  version: string | null
  source: string | null
}

export type PythonEnvStatus = {
  workspace: string
  interpreterPath: string | null
  version: string | null
  tool: PythonInterpreterTool
  isIsolated: boolean
  packagesCount: number | null
  lastUpdatedMs: number
  lastError: string | null
  isPythonProject: boolean
  requiredPython?: RequiredPython
  installRecommendation?: InstallRecommendation
}

export type PythonInterpreterCandidate = {
  path: string
  version: string | null
  source: string
  isVenv: boolean
}

export type PythonProjectMarkers = {
  isPythonProject: boolean
  hasVenvDir: boolean
  hasPyproject: boolean
  hasRequirements: boolean
  hasPipfile: boolean
  hasSetupPy: boolean
  hasSetupCfg: boolean
  hasPythonVersionFile: boolean
  hasUvLock: boolean
}

export type DiscoverResult = {
  interpreters: PythonInterpreterCandidate[]
  markers: PythonProjectMarkers
  requiredPython?: RequiredPython
  installRecommendation?: InstallRecommendation
}

export type PythonCreateBody = {
  workspace: string
  tool?: 'uv' | 'venv'
  pythonVersion?: string
}

export type PythonSelectBody = {
  workspace: string
  interpreterPath: string
}

export type PythonInstallBody = {
  workspace: string
  file?: string
}

export type PythonEnvEvent =
  | {
      kind: 'snapshot'
      state: PythonEnvStatus
    }
  | { kind: 'creating'; workspace: string; tool: string }
  | { kind: 'progress'; workspace: string; message: string }
  | {
      kind: 'ready'
      workspace: string
      interpreter: string
      version: string | null
      fallbackUsed: boolean
    }
  | { kind: 'failed'; workspace: string; error: string }
  | { kind: 'install_start'; workspace: string; file: string }
  | { kind: 'install_progress'; workspace: string; line: string }
  | { kind: 'install_done'; workspace: string; success: boolean; message: string | null }
  | { kind: 'packages_counted'; workspace: string; count: number }
  | { kind: 'purged'; workspace: string }

function encodeWorkspace(ws: string) {
  return encodeURIComponent(ws)
}

export const pythonApi = {
  status(workspace: string) {
    return api.get<PythonEnvStatus>(`/api/python/status?workspace=${encodeWorkspace(workspace)}`)
  },
  discover(workspace: string) {
    return api.get<DiscoverResult>(`/api/python/discover?workspace=${encodeWorkspace(workspace)}`)
  },
  create(body: PythonCreateBody) {
    return api.post<{ accepted: boolean; workspace: string }>('/api/python/create', body)
  },
  select(body: PythonSelectBody) {
    return api.post<PythonEnvStatus>('/api/python/select', body)
  },
  installRequirements(body: PythonInstallBody) {
    return api.post<{ accepted: boolean }>('/api/python/install_requirements', body)
  },
  installSmart(workspace: string) {
    return api.post<{ accepted: boolean }>('/api/python/install', { workspace })
  },
  purge(workspace: string) {
    return api.post<{ success: boolean }>('/api/python/purge', { workspace })
  },
  activation(workspace: string) {
    return api.get<{ env: Record<string, string>; unset: string[] }>(
      `/api/python/activation?workspace=${encodeWorkspace(workspace)}`,
    )
  },
  streamEvents(workspace: string, onEvent: (ev: PythonEnvEvent) => void): EventSource {
    const url = withAuthToken(`${getBaseUrl()}/api/python/events?workspace=${encodeWorkspace(workspace)}`)
    const source = new EventSource(url)
    source.addEventListener('python-env', (msg) => {
      try {
        const parsed = JSON.parse((msg as MessageEvent<string>).data) as PythonEnvEvent
        onEvent(parsed)
      } catch {
      }
    })
    return source
  },
}
