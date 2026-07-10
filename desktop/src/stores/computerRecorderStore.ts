// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { getBaseUrl } from '../api/client'
import {
  deleteRecording,
  generateRecordingSkill,
  listRecordings,
  renameRecording,
  type RecordingSummary,
} from '../api/computer'
import { localizeComputerMessage } from '../lib/computerMessages'
import { useComputerUseStore } from './computerUseStore'

export type RecorderStatus =
  | 'idle'
  | 'recording'
  | 'stopped'
  | 'generating'
  | 'saved'
  | 'error'

export type RecorderStep = {
  index: number
  actionType: string
  elementDescription?: string
  value?: string
  screenshotBase64: string
  targetXNorm?: number
  targetYNorm?: number
  toXNorm?: number
  toYNorm?: number
}

type ComputerRecorderStore = {
  status: RecorderStatus
  error: string | null
  statusMessage: string | null
  task: string
  savedRecordingName: string | null
  savedSkillName: string | null
  generatingNames: string[]
  steps: RecorderStep[]
  selectedStepIndex: number | null
  startedAt: number | null
  recordings: RecordingSummary[]
  recordingsLoaded: boolean

  setTask: (task: string) => void
  selectStep: (index: number | null) => void
  startRecording: () => void
  stopRecording: () => void
  discardRecording: () => void
  generateSkill: () => void
  generateForRecording: (name: string) => Promise<void>
  loadRecordings: () => Promise<void>
  removeRecording: (name: string) => Promise<void>
  renameRecording: (name: string, newName: string) => Promise<boolean>
  reset: () => void
}

let socket: WebSocket | null = null

function closeSocket() {
  if (socket) {
    try {
      socket.onclose = null
      socket.onmessage = null
      socket.onerror = null
      socket.onopen = null
      socket.close()
    } catch {
      /* ignore */
    }
    socket = null
  }
}

function genRecId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID()
    }
  } catch {
    /* ignore */
  }
  return `rec-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

export const useComputerRecorderStore = create<ComputerRecorderStore>((set, get) => ({
  status: 'idle',
  error: null,
  statusMessage: null,
  task: '',
  savedRecordingName: null,
  savedSkillName: null,
  generatingNames: [],
  steps: [],
  selectedStepIndex: null,
  startedAt: null,
  recordings: [],
  recordingsLoaded: false,

  setTask: (task) => set({ task }),
  selectStep: (index) => set({ selectedStepIndex: index }),

  startRecording: () => {
    const { status } = get()
    if (status === 'recording') return

    if (socket && socket.readyState === WebSocket.OPEN) {
      try {
        socket.send(JSON.stringify({ type: 'discard' }))
      } catch {
        /* ignore */
      }
    }
    closeSocket()
    const recId = genRecId()
    set({
      status: 'idle',
      error: null,
      statusMessage: null,
      steps: [],
      selectedStepIndex: null,
      savedRecordingName: null,
      savedSkillName: null,
      startedAt: null,
    })

    const wsUrl = `${getBaseUrl().replace(/^http/, 'ws')}/ws/computer-record/${recId}`
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
      ws.send(JSON.stringify({ type: 'start', task: get().task.trim() }))
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
      if (current === 'recording') {
        set({ status: 'error', error: 'connection lost; the recording was discarded' })
      } else if (current === 'generating') {
        const name = get().savedRecordingName
        if (!name) {
          set({ status: 'error', error: 'connection lost during skill generation' })
          return
        }
        void (async () => {
          const done = await pollSkillGenerated(name, get)
          if (get().savedRecordingName !== name || get().status !== 'generating') return
          if (done) {
            set({ status: 'saved', savedSkillName: name })
          } else {
            set({
              status: 'error',
              error: 'connection lost during skill generation; retry from the skill library',
            })
          }
        })()
      }
    }
  },

  stopRecording: () => {
    try {
      socket?.send(JSON.stringify({ type: 'stop' }))
    } catch {
      /* ignore */
    }
  },

  discardRecording: () => {
    try {
      socket?.send(JSON.stringify({ type: 'discard' }))
    } catch {
      /* ignore */
    }
    closeSocket()
    set({
      status: 'idle',
      steps: [],
      selectedStepIndex: null,
      statusMessage: null,
      error: null,
      startedAt: null,
      savedRecordingName: null,
      savedSkillName: null,
    })
  },

  generateSkill: () => {
    const { savedRecordingName, status } = get()
    if (!savedRecordingName || status === 'generating') return
    const { provider, model } = useComputerUseStore.getState()
    if (socket && socket.readyState === WebSocket.OPEN) {
      set({ status: 'generating', error: null })
      try {
        socket.send(
          JSON.stringify({
            type: 'generate_saved',
            name: savedRecordingName,
            provider: provider ?? undefined,
            model: model ?? undefined,
          }),
        )
      } catch {
        set({ status: 'error', error: 'failed to send generate request' })
      }
      return
    }
    set({ status: 'generating', error: null })
    void (async () => {
      try {
        await generateRecordingSkill(savedRecordingName, provider ?? undefined, model ?? undefined)
        const done = await pollSkillGenerated(savedRecordingName, get)
        if (get().savedRecordingName !== savedRecordingName) return
        if (done) {
          set({ status: 'saved', savedSkillName: savedRecordingName })
        } else {
          set({ status: 'error', error: 'skill generation timed out; check logs and retry' })
        }
      } catch (err) {
        if (get().savedRecordingName !== savedRecordingName) return
        set({
          status: 'error',
          error: err instanceof Error ? err.message : 'failed to start skill generation',
        })
      }
    })()
  },

  generateForRecording: async (name) => {
    const { generatingNames } = get()
    if (generatingNames.includes(name)) return
    const { provider, model } = useComputerUseStore.getState()
    set({ generatingNames: [...get().generatingNames, name], error: null })
    try {
      await generateRecordingSkill(name, provider ?? undefined, model ?? undefined)
      await pollSkillGenerated(name, get)
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'failed to start skill generation' })
    } finally {
      set({ generatingNames: get().generatingNames.filter((n) => n !== name) })
      void get().loadRecordings()
    }
  },

  loadRecordings: async () => {
    try {
      const recordings = await listRecordings()
      set({ recordings, recordingsLoaded: true })
    } catch (err) {
      set({
        recordingsLoaded: true,
        error: err instanceof Error ? err.message : 'failed to load recordings',
      })
    }
  },

  removeRecording: async (name) => {
    try {
      await deleteRecording(name)
      set({ recordings: get().recordings.filter((r) => r.name !== name) })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'failed to delete recording' })
    }
  },

  renameRecording: async (name, newName) => {
    try {
      const renamed = await renameRecording(name, newName)
      set({
        recordings: get().recordings.map((r) =>
          r.name === name ? { ...r, name: renamed } : r,
        ),
        error: null,
      })
      if (get().savedRecordingName === name) {
        set({ savedRecordingName: renamed })
      }
      return true
    } catch (err) {
      set({ error: err instanceof Error ? err.message : 'failed to rename recording' })
      return false
    }
  },

  reset: () => {
    closeSocket()
    set({
      status: 'idle',
      error: null,
      statusMessage: null,
      steps: [],
      selectedStepIndex: null,
      startedAt: null,
      savedRecordingName: null,
      savedSkillName: null,
    })
  },
}))

async function pollSkillGenerated(
  name: string,
  get: () => ComputerRecorderStore,
): Promise<boolean> {
  const POLL_INTERVAL_MS = 3_000
  const MAX_WAIT_MS = 5 * 60_000
  const deadline = Date.now() + MAX_WAIT_MS
  while (Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS))
    try {
      const recordings = await listRecordings()
      const rec = recordings.find((r) => r.name === name)
      if (!rec) return false
      if (rec.has_skill) {
        void get().loadRecordings()
        return true
      }
    } catch {
      /* transient; keep polling */
    }
  }
  return false
}

function handleEvent(
  payload: Record<string, unknown>,
  set: (partial: Partial<ComputerRecorderStore>) => void,
  get: () => ComputerRecorderStore,
) {
  const type = typeof payload.type === 'string' ? payload.type : ''
  switch (type) {
    case 'status': {
      const status = (payload.status as RecorderStatus) ?? 'idle'
      const rawMessage = typeof payload.message === 'string' ? payload.message : null
      const code = typeof payload.code === 'string' ? payload.code : null
      const message = localizeComputerMessage(code, rawMessage)
      const patch: Partial<ComputerRecorderStore> = { status, statusMessage: message }
      if (status === 'recording' && get().startedAt === null) {
        patch.startedAt = Date.now()
      }
      set(patch)
      break
    }
    case 'step': {
      const MAX_FULL_SCREENSHOTS = 8
      const step: RecorderStep = {
        index: Number(payload.index ?? 0),
        actionType: String(payload.action_type ?? ''),
        elementDescription:
          typeof payload.element_description === 'string'
            ? payload.element_description
            : undefined,
        value: typeof payload.value === 'string' ? payload.value : undefined,
        screenshotBase64: String(payload.screenshot_base64 ?? ''),
        targetXNorm: typeof payload.target_x_norm === 'number' ? payload.target_x_norm : undefined,
        targetYNorm: typeof payload.target_y_norm === 'number' ? payload.target_y_norm : undefined,
        toXNorm: typeof payload.to_x_norm === 'number' ? payload.to_x_norm : undefined,
        toYNorm: typeof payload.to_y_norm === 'number' ? payload.to_y_norm : undefined,
      }
      const steps = [...get().steps, step]
      const cutoff = steps.length - MAX_FULL_SCREENSHOTS
      for (let i = 0; i < cutoff; i++) {
        const existing = steps[i]
        if (existing && existing.screenshotBase64) {
          steps[i] = { ...existing, screenshotBase64: '' }
        }
      }
      set({ steps, selectedStepIndex: steps.length - 1 })
      break
    }
    case 'recording_saved': {
      const name = typeof payload.name === 'string' ? payload.name : null
      set({ savedRecordingName: name })
      void get().loadRecordings()
      break
    }
    case 'skill_saved': {
      const name = typeof payload.name === 'string' ? payload.name : null
      set({ savedSkillName: name })
      void get().loadRecordings()
      break
    }
    case 'error': {
      const rawMessage = typeof payload.message === 'string' ? payload.message : 'unknown error'
      const code = typeof payload.code === 'string' ? payload.code : null
      set({ error: localizeComputerMessage(code, rawMessage) })
      break
    }
    default:
      break
  }
}
