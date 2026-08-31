// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { getBaseUrl, withAuthToken } from '../api/client'
import { listVisionModels, stopComputerRun, type VisionModel } from '../api/computer'
import { computerText, localizeComputerMessage } from '../lib/computerMessages'

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
  uid: number
  kind: 'action' | 'user_update'
  index: number
  thought: string
  actionType: string
  elementDescription?: string
  value?: string
  screenshotBase64: string
  screenshotMime?: string
  targetXNorm?: number
  targetYNorm?: number
  toXNorm?: number
  toYNorm?: number
  confidence?: number
  success?: boolean
  resultMessage?: string
}

export type ComputerAttachment = {
  name: string
  mime: string
  dataBase64?: string
  text?: string
}

type StoredSelection = { provider: string; model: string }
type StoredParams = { maxSteps: number; stepDelayMs: number }

export type StartOptions = {
  skill?: string
  replayRecording?: string
  taskOverride?: string
  smart?: boolean
  attachments?: ComputerAttachment[]
  repeat?: { count: number; intervalMs: number }
}

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
  pendingSteer: string | null

  loadModels: () => Promise<void>
  setSelection: (provider: string, model: string) => void
  setMaxSteps: (value: number) => void
  setStepDelayMs: (value: number) => void
  setTask: (task: string) => void
  selectStep: (index: number | null) => void
  start: (options?: StartOptions) => boolean
  send: (text: string, attachments?: ComputerAttachment[]) => boolean
  steer: (text: string, attachments?: ComputerAttachment[]) => boolean
  stop: () => void
  sendReply: (text: string) => void
  reset: () => void
}

let socket: WebSocket | null = null
let activeRunId: string | null = null
let nextStepUid = 1
let queuedSteer: { text: string; attachments?: ComputerAttachment[] } | null = null

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
  queuedSteer = null
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

const isBusy = (status: ComputerStatus) =>
  status === 'running' || status === 'thinking' || status === 'connecting'

function wireAttachments(attachments?: ComputerAttachment[]) {
  if (!attachments || attachments.length === 0) return undefined
  return attachments.map((a) => ({
    name: a.name,
    mime: a.mime,
    ...(a.dataBase64 ? { data_base64: a.dataBase64 } : {}),
    ...(a.text ? { text: a.text } : {}),
  }))
}

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
    pendingSteer: null,

    loadModels: async () => {
      try {
        const models = await listVisionModels()
        set({ models, modelsLoaded: true })
        const { provider, model } = get()
        const hasSelection =
          provider && model && models.some((m) => m.provider === provider && m.model === model)
        if (models.length === 0) {
          if (provider || model) {
            set({ provider: null, model: null })
            try {
              localStorage.removeItem(SELECTION_KEY)
            } catch {  }
          }
          return
        }
        const preferred = models.find((m) => m.recommended) ?? models[0]
        if (!hasSelection && preferred) {
          set({ provider: preferred.provider, model: preferred.model })
          try {
            localStorage.setItem(
              SELECTION_KEY,
              JSON.stringify({ provider: preferred.provider, model: preferred.model }),
            )
          } catch {  }
        }
      } catch (err) {
        set({
          modelsLoaded: false,
          error:
            err instanceof Error
              ? err.message
              : computerText('computerUse.msg.visionModelsLoadFailed'),
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

    start: (options) => {
      const { task, provider, model, maxSteps, stepDelayMs, status } = get()
      const isReplay = Boolean(options?.replayRecording)
      const isSkill = Boolean(options?.skill)
      if (!isReplay && (!provider || !model)) return false
      if (!isReplay && !isSkill && !task.trim()) return false
      if (isBusy(status) || status === 'call_user') return false

      closeSocket()
      const runId = genRunId()
      activeRunId = runId
      set({
        status: 'connecting',
        statusMessage: null,
        error: null,
        steps: [],
        selectedStepIndex: null,
        pendingSteer: null,
      })

      const wsUrl = withAuthToken(`${getBaseUrl().replace(/^http/, 'ws')}/ws/computer/${runId}`)
      let ws: WebSocket
      try {
        ws = new WebSocket(wsUrl)
      } catch (err) {
        set({
          status: 'error',
          error:
            err instanceof Error
              ? err.message
              : computerText('computerUse.msg.connectionOpenFailed'),
        })
        return true
      }
      socket = ws

      ws.onopen = () => {
        if (socket !== ws) return
        set({ status: 'running' })
        const flushSteer = () => {
          if (!queuedSteer) return
          const pending = queuedSteer
          queuedSteer = null
          get().steer(pending.text, pending.attachments)
        }
        if (isReplay) {
          const smart = Boolean(options?.smart) && Boolean(provider && model)
          ws.send(
            JSON.stringify({
              type: 'start',
              mode: 'replay',
              recording: options?.replayRecording,
              ...(options?.repeat ? { repeat: options.repeat } : {}),
              ...(smart || provider
                ? {
                    provider: provider ?? undefined,
                    model: model ?? undefined,
                  }
                : {}),
              ...(smart ? { smart: true } : {}),
            }),
          )
          flushSteer()
          return
        }
        const effectiveTask = isSkill
          ? (options?.taskOverride ?? '').trim() ||
            'Run the recorded skill as described using the recorded steps.'
          : task.trim()
        ws.send(
          JSON.stringify({
            type: 'start',
            task: effectiveTask,
            provider,
            model,
            maxSteps,
            stepDelayMs,
            ...(isSkill ? { skill: options?.skill } : {}),
            ...(options?.attachments
              ? { attachments: wireAttachments(options.attachments) }
              : {}),
          }),
        )
        flushSteer()
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
        if (!isTerminal(current)) {
          set({ status: 'stopped', pendingSteer: null })
        }
      }
      return true
    },

    steer: (text, attachments) => {
      const trimmed = text.trim()
      if (!trimmed && (!attachments || attachments.length === 0)) return false
      if (socket && socket.readyState === WebSocket.CONNECTING) {
        queuedSteer = { text: trimmed, attachments }
        set({ pendingSteer: trimmed || null })
        return true
      }
      if (!socket || socket.readyState !== WebSocket.OPEN) return false
      try {
        socket.send(
          JSON.stringify({
            type: 'steer',
            text: trimmed,
            provider: get().provider ?? undefined,
            model: get().model ?? undefined,
            ...(attachments ? { attachments: wireAttachments(attachments) } : {}),
          }),
        )
        set({ pendingSteer: trimmed || null })
        return true
      } catch {
        return false
      }
    },

    send: (text, attachments) => {
      const state = get()
      const trimmed = text.trim()
      if (!trimmed && (!attachments || attachments.length === 0)) return false
      if (state.status === 'call_user') {
        if (!socket || socket.readyState !== WebSocket.OPEN) {
          set({ status: 'stopped', error: computerText('computerUse.msg.connectionLostRun') })
          closeSocket()
          return false
        }
        try {
          socket.send(
            JSON.stringify({
              type: 'user_reply',
              text: trimmed,
              ...(attachments ? { attachments: wireAttachments(attachments) } : {}),
            }),
          )
          set({ status: 'running', pendingSteer: trimmed || null })
          return true
        } catch {
          return false
        }
      }
      if (isBusy(state.status)) {
        return get().steer(trimmed, attachments)
      }
      if (!trimmed) return false
      set({ task: trimmed })
      return get().start({ attachments })
    },

    stop: () => {
      const runId = activeRunId
      try {
        socket?.send(JSON.stringify({ type: 'stop' }))
      } catch {  }
      if (runId) {
        void stopComputerRun(runId).catch(() => {})
      }
      set({ status: 'stopped', pendingSteer: null })
      closeSocket()
    },

    sendReply: (text) => {
      const trimmed = text.trim()
      if (!trimmed) return
      if (!socket || socket.readyState !== WebSocket.OPEN) {
        set({ status: 'stopped', error: computerText('computerUse.msg.connectionLostRun') })
        closeSocket()
        return
      }
      try {
        socket.send(JSON.stringify({ type: 'user_reply', text: trimmed }))
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
        pendingSteer: null,
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
      const rawMessage = typeof payload.message === 'string' ? payload.message : null
      const code = typeof payload.code === 'string' ? payload.code : null
      const message = localizeComputerMessage(code, rawMessage)
      set({ status, statusMessage: message })
      if (isTerminal(status)) {
        set({ pendingSteer: null })
        closeSocket()
      }
      break
    }
    case 'step': {
      const MAX_FULL_SCREENSHOTS = 10
      const step: ComputerStep = {
        uid: nextStepUid++,
        kind: 'action',
        index: Number(payload.index ?? 0),
        thought: String(payload.thought ?? ''),
        actionType: String(payload.action_type ?? ''),
        elementDescription:
          typeof payload.element_description === 'string'
            ? payload.element_description
            : undefined,
        value: typeof payload.value === 'string' ? payload.value : undefined,
        screenshotBase64: String(payload.screenshot_base64 ?? ''),
        screenshotMime:
          typeof payload.screenshot_mime === 'string' ? payload.screenshot_mime : undefined,
        targetXNorm:
          typeof payload.target_x_norm === 'number' ? payload.target_x_norm : undefined,
        targetYNorm:
          typeof payload.target_y_norm === 'number' ? payload.target_y_norm : undefined,
        toXNorm: typeof payload.to_x_norm === 'number' ? payload.to_x_norm : undefined,
        toYNorm: typeof payload.to_y_norm === 'number' ? payload.to_y_norm : undefined,
        confidence: typeof payload.confidence === 'number' ? payload.confidence : undefined,
      }
      const state = get()
      let lastActionIndex = -1
      for (let i = state.steps.length - 1; i >= 0; i--) {
        const existing = state.steps[i]
        if (existing && existing.kind === 'action') {
          lastActionIndex = i
          break
        }
      }
      const following =
        state.selectedStepIndex === null || state.selectedStepIndex >= lastActionIndex
      const steps = [...state.steps, step]
      let withShots = 0
      for (let i = steps.length - 1; i >= 0; i--) {
        const existing = steps[i]
        if (!existing || !existing.screenshotBase64) continue
        withShots += 1
        if (withShots > MAX_FULL_SCREENSHOTS) {
          steps[i] = { ...existing, screenshotBase64: '' }
        }
      }
      set({
        steps,
        selectedStepIndex: following ? steps.length - 1 : state.selectedStepIndex,
      })
      break
    }
    case 'user_update': {
      const text = typeof payload.text === 'string' ? payload.text : ''
      if (!text) {
        set({ pendingSteer: null })
        break
      }
      const step: ComputerStep = {
        uid: nextStepUid++,
        kind: 'user_update',
        index: Number(payload.index ?? 0),
        thought: text,
        actionType: 'user_update',
        screenshotBase64: '',
      }
      const steps = [...get().steps, step]
      set({ steps, pendingSteer: null })
      break
    }
    case 'action_result': {
      const index = Number(payload.index ?? -1)
      const success = Boolean(payload.success)
      const message = typeof payload.message === 'string' ? payload.message : undefined
      const steps = [...get().steps]
      for (let i = steps.length - 1; i >= 0; i--) {
        const s = steps[i]
        if (s && s.kind === 'action' && s.index === index && s.success === undefined) {
          steps[i] = { ...s, success, resultMessage: message }
          break
        }
      }
      set({ steps })
      break
    }
    case 'error': {
      const rawMessage = typeof payload.message === 'string' ? payload.message : null
      const code = typeof payload.code === 'string' ? payload.code : null
      set({
        error:
          localizeComputerMessage(code, rawMessage) ??
          computerText('computerUse.msg.unknownError'),
      })
      break
    }
    default:
      break
  }
}
