// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { getBaseUrl } from '../api/client'
import { listVisionModels, stopComputerRun, type VisionModel } from '../api/computer'

const SELECTION_KEY = 'sen-computer-selection'
const PARAMS_KEY = 'sen-computer-params'

export type ComputerStatus =
  | 'idle'
  | 'connecting'
  | 'running'
  | 'thinking'
  | 'finished'
  | 'call_user'
  | 'error'
  | 'stopped'

export type ComputerStep = {
  index: number
  thought: string
  actionType: string
  elementDescription?: string
  value?: string
  screenshotBase64: string
  targetXNorm?: number
  targetYNorm?: number
  toXNorm?: number
  toYNorm?: number
  confidence?: number
  success?: boolean
  resultMessage?: string
}

type StoredSelection = { provider: string; model: string }
type StoredParams = { maxSteps: number; stepDelayMs: number }

function loadSelection(): StoredSelection | null {
  try {
    const raw = localStorage.getItem(SELECTION_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as StoredSelection
    if (parsed && typeof parsed.provider === 'string' && typeof parsed.model === 'string') {
      return parsed
    }
  } catch {  }
  return null
}

function loadParams(): StoredParams {
  try {
    const raw = localStorage.getItem(PARAMS_KEY)
    if (raw) {
      const parsed = JSON.parse(raw) as StoredParams
      if (parsed && Number.isFinite(parsed.maxSteps) && Number.isFinite(parsed.stepDelayMs)) {
        return {
          maxSteps: Math.min(200, Math.max(1, Math.round(parsed.maxSteps))),
          stepDelayMs: Math.min(10_000, Math.max(0, Math.round(parsed.stepDelayMs))),
        }
      }
    }
  } catch {  }
  return { maxSteps: 40, stepDelayMs: 600 }
}

type ComputerUseStore = {
  models: VisionModel[]
  modelsLoaded: boolean
  provider: string | null
  model: string | null
  maxSteps: number
  stepDelayMs: number
  task: string
  status: ComputerStatus
  statusMessage: string | null
  error: string | null
  steps: ComputerStep[]
  selectedStepIndex: number | null

  loadModels: () => Promise<void>
  setSelection: (provider: string, model: string) => void
  setMaxSteps: (value: number) => void
  setStepDelayMs: (value: number) => void
  setTask: (task: string) => void
  selectStep: (index: number | null) => void
  start: () => void
  stop: () => void
  sendReply: (text: string) => void
  reset: () => void
}

let socket: WebSocket | null = null
let activeRunId: string | null = null

function closeSocket() {
  if (socket) {
    try {
      socket.onclose = null
      socket.onmessage = null
      socket.onerror = null
      socket.onopen = null
      socket.close()
    } catch {  }
    socket = null
  }
}

function genRunId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID()
    }
  } catch {  }
  return `run-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

const isTerminal = (status: ComputerStatus) =>
  status === 'finished' || status === 'error' || status === 'stopped'

export const useComputerUseStore = create<ComputerUseStore>((set, get) => {
  const initialSelection = loadSelection()
  const initialParams = loadParams()
  return {
    models: [],
    modelsLoaded: false,
    provider: initialSelection?.provider ?? null,
    model: initialSelection?.model ?? null,
    maxSteps: initialParams.maxSteps,
    stepDelayMs: initialParams.stepDelayMs,
    task: '',
    status: 'idle',
    statusMessage: null,
    error: null,
    steps: [],
    selectedStepIndex: null,

    loadModels: async () => {
      try {
        const models = await listVisionModels()
        set({ models, modelsLoaded: true })
        const { provider, model } = get()
        const hasSelection =
          provider && model && models.some((m) => m.provider === provider && m.model === model)
        const preferred = models.find((m) => m.recommended) ?? models[0]
        if (!hasSelection && preferred) {
          set({ provider: preferred.provider, model: preferred.model })
        }
      } catch (err) {
        set({
          modelsLoaded: true,
          error: err instanceof Error ? err.message : 'failed to load vision models',
        })
      }
    },

    setSelection: (provider, model) => {
      try {
        localStorage.setItem(SELECTION_KEY, JSON.stringify({ provider, model }))
      } catch {  }
      set({ provider, model })
    },

    setMaxSteps: (value) => {
      const clamped = Math.min(200, Math.max(1, Math.round(value || 0)))
      set({ maxSteps: clamped })
      try {
        localStorage.setItem(
          PARAMS_KEY,
          JSON.stringify({ maxSteps: clamped, stepDelayMs: get().stepDelayMs }),
        )
      } catch {  }
    },

    setStepDelayMs: (value) => {
      const clamped = Math.min(10_000, Math.max(0, Math.round(value || 0)))
      set({ stepDelayMs: clamped })
      try {
        localStorage.setItem(
          PARAMS_KEY,
          JSON.stringify({ maxSteps: get().maxSteps, stepDelayMs: clamped }),
        )
      } catch {  }
    },

    setTask: (task) => set({ task }),

    selectStep: (index) => set({ selectedStepIndex: index }),

    start: () => {
      const { task, provider, model, maxSteps, stepDelayMs, status } = get()
      if (!task.trim() || !provider || !model) return
      if (status === 'running' || status === 'thinking' || status === 'connecting') return

      closeSocket()
      const runId = genRunId()
      activeRunId = runId
      set({
        status: 'connecting',
        statusMessage: null,
        error: null,
        steps: [],
        selectedStepIndex: null,
      })

      const wsUrl = `${getBaseUrl().replace(/^http/, 'ws')}/ws/computer/${runId}`
      let ws: WebSocket
      try {
        ws = new WebSocket(wsUrl)
      } catch (err) {
        set({
          status: 'error',
          error: err instanceof Error ? err.message : 'failed to open connection',
        })
        return
      }
      socket = ws

      ws.onopen = () => {
        if (socket !== ws) return
        set({ status: 'running' })
        ws.send(
          JSON.stringify({
            type: 'start',
            task: task.trim(),
            provider,
            model,
            maxSteps,
            stepDelayMs,
          }),
        )
      }

      ws.onmessage = (event) => {
        if (socket !== ws) return
        let payload: Record<string, unknown>
        try {
          payload = JSON.parse(event.data as string)
        } catch {
          return
        }
        handleEvent(payload, set, get)
      }

      ws.onerror = () => {
        if (socket !== ws) return
      }

      ws.onclose = () => {
        if (socket !== ws) return
        socket = null
        const current = get().status
        if (!isTerminal(current) && current !== 'call_user') {
          set({ status: 'stopped' })
        }
      }
    },

    stop: () => {
      const runId = activeRunId
      try {
        socket?.send(JSON.stringify({ type: 'stop' }))
      } catch {  }
      if (runId) {
        void stopComputerRun(runId).catch(() => {})
      }
      set({ status: 'stopped' })
      closeSocket()
    },

    sendReply: (text) => {
      const trimmed = text.trim()
      if (!trimmed) return
      try {
        socket?.send(JSON.stringify({ type: 'user_reply', text: trimmed }))
        set({ status: 'running' })
      } catch {  }
    },

    reset: () => {
      closeSocket()
      activeRunId = null
      set({
        status: 'idle',
        statusMessage: null,
        error: null,
        steps: [],
        selectedStepIndex: null,
      })
    },
  }
})

function handleEvent(
  payload: Record<string, unknown>,
  set: (partial: Partial<ComputerUseStore>) => void,
  get: () => ComputerUseStore,
) {
  const type = typeof payload.type === 'string' ? payload.type : ''
  switch (type) {
    case 'status': {
      const status = (payload.status as ComputerStatus) ?? 'running'
      const message = typeof payload.message === 'string' ? payload.message : null
      set({ status, statusMessage: message })
      if (isTerminal(status)) {
        closeSocket()
      }
      break
    }
    case 'step': {
      const step: ComputerStep = {
        index: Number(payload.index ?? 0),
        thought: String(payload.thought ?? ''),
        actionType: String(payload.action_type ?? ''),
        elementDescription:
          typeof payload.element_description === 'string'
            ? payload.element_description
            : undefined,
        value: typeof payload.value === 'string' ? payload.value : undefined,
        screenshotBase64: String(payload.screenshot_base64 ?? ''),
        targetXNorm:
          typeof payload.target_x_norm === 'number' ? payload.target_x_norm : undefined,
        targetYNorm:
          typeof payload.target_y_norm === 'number' ? payload.target_y_norm : undefined,
        toXNorm: typeof payload.to_x_norm === 'number' ? payload.to_x_norm : undefined,
        toYNorm: typeof payload.to_y_norm === 'number' ? payload.to_y_norm : undefined,
        confidence: typeof payload.confidence === 'number' ? payload.confidence : undefined,
      }
      const steps = [...get().steps, step]
      set({ steps, selectedStepIndex: steps.length - 1 })
      break
    }
    case 'action_result': {
      const index = Number(payload.index ?? -1)
      const success = Boolean(payload.success)
      const message = typeof payload.message === 'string' ? payload.message : undefined
      const steps = get().steps.map((s) =>
        s.index === index ? { ...s, success, resultMessage: message } : s,
      )
      set({ steps })
      break
    }
    case 'error': {
      const message = typeof payload.message === 'string' ? payload.message : 'unknown error'
      set({ error: message })
      break
    }
    default:
      break
  }
}
