// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { getBaseUrl, withAuthToken } from '../api/client'
import {
  getAnalysis,
  getSensitiveReport,
  listBuildTargets,
  saveAnalysis,
  type Analysis,
  type AnalysisStep,
  type BuildTarget,
  type SensitiveReport,
} from '../api/computer'
import { computerText, localizeComputerMessage } from '../lib/computerMessages'
import { useComputerUseStore } from './computerUseStore'

export type AnalyzePhase = 'idle' | 'running' | 'done' | 'error'
export type BuildPhase = 'idle' | 'planning' | 'plan' | 'creating' | 'done' | 'error'

export type SkillPlan = {
  architecture: string
  name: string
  title: string
  description: string
  summary: string
  generalization: string
  values: { id: string; name: string; value: string }[]
  steps: { title: string; text: string; kind: string; tool: string }[]
  allowedTools: string[]
}

export type AutomationPlan = {
  architecture: string
  name: string
  title: string
  description: string
  summary: string
  generalization: string
  triggerType: string
  schedule: Record<string, unknown>
  condition: string
  values: { id: string; name: string; value: string }[]
  steps: { label: string; prompt: string }[]
  model: string
}

type Store = {
  activeRecording: string | null
  analysis: Analysis | null
  sensitiveReport: SensitiveReport | null
  analyzePhase: AnalyzePhase
  analyzeMessage: string | null
  analyzeError: string | null
  redactedCount: number | null

  targets: BuildTarget[]
  buildKind: 'skill' | 'automation'
  buildArchitecture: string
  buildPhase: BuildPhase
  buildMessage: string | null
  buildError: string | null
  skillPlan: SkillPlan | null
  automationPlan: AutomationPlan | null
  builtPath: string | null
  builtPlacement: string | null

  open: (recording: string) => Promise<void>
  close: () => void
  startAnalyze: () => void
  sendFeedback: (overall: string, steps: { stepId: string; note: string }[]) => void
  cancelAnalyze: () => void
  persistAnalysisEdit: (patch: { title?: string; intent?: string; steps?: AnalysisStep[] }) => Promise<void>
  approveAnalysis: () => Promise<void>

  loadTargets: () => Promise<void>
  setBuildTarget: (kind: 'skill' | 'automation', architecture: string) => void
  propose: () => void
  refine: (feedback: string) => void
  updateSkillPlan: (plan: SkillPlan) => void
  updateAutomationPlan: (plan: AutomationPlan) => void
  create: (placement: string, exportDir?: string) => void
  resetBuild: () => void
}

let analyzeSocket: WebSocket | null = null
let buildSocket: WebSocket | null = null

function closeSocket(socket: WebSocket | null) {
  if (!socket) return
  try {
    socket.onclose = null
    socket.onmessage = null
    socket.onerror = null
    socket.onopen = null
    socket.close()
  } catch {
  }
}

function wsUrl(path: string): string {
  return withAuthToken(`${getBaseUrl().replace(/^http/, 'ws')}${path}`)
}

export const useComputerAnalysisStore = create<Store>((set, get) => ({
  activeRecording: null,
  analysis: null,
  sensitiveReport: null,
  analyzePhase: 'idle',
  analyzeMessage: null,
  analyzeError: null,
  redactedCount: null,

  targets: [],
  buildKind: 'skill',
  buildArchitecture: 'sen-agent',
  buildPhase: 'idle',
  buildMessage: null,
  buildError: null,
  skillPlan: null,
  automationPlan: null,
  builtPath: null,
  builtPlacement: null,

  open: async (recording) => {
    closeSocket(analyzeSocket)
    closeSocket(buildSocket)
    analyzeSocket = null
    buildSocket = null
    set({
      activeRecording: recording,
      analysis: null,
      sensitiveReport: null,
      analyzePhase: 'idle',
      analyzeMessage: null,
      analyzeError: null,
      redactedCount: null,
      buildPhase: 'idle',
      buildMessage: null,
      buildError: null,
      skillPlan: null,
      automationPlan: null,
      builtPath: null,
      builtPlacement: null,
    })
    try {
      const { analysis, sensitiveReport } = await getAnalysis(recording)
      set({ analysis, sensitiveReport })
    } catch {
    }
    void get().loadTargets()
  },

  close: () => {
    closeSocket(analyzeSocket)
    closeSocket(buildSocket)
    analyzeSocket = null
    buildSocket = null
    set({ activeRecording: null })
  },

  startAnalyze: () => {
    const recording = get().activeRecording
    if (!recording) return
    runAnalyzeSocket(recording, { type: 'start' }, set, get)
  },

  sendFeedback: (overall, steps) => {
    const recording = get().activeRecording
    if (!recording) return
    runAnalyzeSocket(recording, { type: 'feedback', overall, steps }, set, get)
  },

  cancelAnalyze: () => {
    try {
      analyzeSocket?.send(JSON.stringify({ type: 'cancel' }))
    } catch {
    }
    set({ analyzePhase: 'idle', analyzeMessage: null })
  },

  persistAnalysisEdit: async (patch) => {
    const recording = get().activeRecording
    if (!recording) return
    try {
      const analysis = await saveAnalysis(recording, patch)
      set({ analysis })
    } catch (err) {
      set({
        analyzeError:
          err instanceof Error
            ? err.message
            : computerText('computerUse.msg.analysisSaveFailed'),
      })
    }
  },

  approveAnalysis: async () => {
    const recording = get().activeRecording
    if (!recording) return
    try {
      const analysis = await saveAnalysis(recording, { approved: true })
      set({ analysis })
    } catch (err) {
      set({
        analyzeError:
          err instanceof Error
            ? err.message
            : computerText('computerUse.msg.analysisSaveFailed'),
      })
    }
  },

  loadTargets: async () => {
    if (get().targets.length > 0) return
    try {
      const targets = await listBuildTargets()
      set({ targets })
    } catch {
    }
  },

  setBuildTarget: (kind, architecture) =>
    set({
      buildKind: kind,
      buildArchitecture: architecture,
      buildPhase: 'idle',
      skillPlan: null,
      automationPlan: null,
      builtPath: null,
      builtPlacement: null,
      buildError: null,
    }),

  propose: () => {
    const recording = get().activeRecording
    if (!recording) return
    const { buildKind, buildArchitecture } = get()
    const { provider, model } = useComputerUseStore.getState()
    runBuildSocket(
      recording,
      {
        type: 'propose',
        kind: buildKind,
        architecture: buildArchitecture,
        provider: provider ?? undefined,
        model: model ?? undefined,
      },
      set,
      get,
    )
  },

  refine: (feedback) => {
    const recording = get().activeRecording
    if (!recording) return
    const { buildKind, buildArchitecture } = get()
    const { provider, model } = useComputerUseStore.getState()
    runBuildSocket(
      recording,
      {
        type: 'refine',
        kind: buildKind,
        architecture: buildArchitecture,
        feedback,
        provider: provider ?? undefined,
        model: model ?? undefined,
      },
      set,
      get,
    )
  },

  updateSkillPlan: (plan) => set({ skillPlan: plan }),
  updateAutomationPlan: (plan) => set({ automationPlan: plan }),

  create: (placement, exportDir) => {
    const recording = get().activeRecording
    if (!recording) return
    const { buildKind, skillPlan, automationPlan } = get()
    const plan = buildKind === 'automation' ? automationPlan : skillPlan
    if (!plan) return
    const { provider, model } = useComputerUseStore.getState()
    runBuildSocket(
      recording,
      {
        type: 'create',
        placement,
        exportDir,
        plan,
        provider: provider ?? undefined,
        model: model ?? undefined,
      },
      set,
      get,
      true,
    )
  },

  resetBuild: () =>
    set({
      buildPhase: 'idle',
      buildMessage: null,
      buildError: null,
      skillPlan: null,
      automationPlan: null,
      builtPath: null,
      builtPlacement: null,
    }),
}))

function runAnalyzeSocket(
  recording: string,
  message: Record<string, unknown>,
  set: (partial: Partial<Store>) => void,
  get: () => Store,
) {
  const { provider, model } = useComputerUseStore.getState()
  if (!provider || !model) {
    set({
      analyzePhase: 'error',
      analyzeError: computerText('computerUse.msg.noVisionModel'),
      analyzeMessage: null,
    })
    return
  }
  closeSocket(analyzeSocket)
  set({
    analyzePhase: 'running',
    analyzeError: null,
    analyzeMessage: null,
    redactedCount: null,
  })
  let ws: WebSocket
  try {
    ws = new WebSocket(wsUrl('/ws/computer-analyze/session'))
  } catch (err) {
    set({
      analyzePhase: 'error',
      analyzeError:
        err instanceof Error
          ? err.message
          : computerText('computerUse.msg.connectionOpenFailed'),
    })
    return
  }
  analyzeSocket = ws
  ws.onopen = () => {
    ws.send(
      JSON.stringify({
        ...message,
        recording,
        provider: provider ?? undefined,
        model: model ?? undefined,
      }),
    )
  }
  ws.onmessage = (event) => {
    if (analyzeSocket !== ws) return
    let payload: Record<string, unknown>
    try {
      payload = JSON.parse(event.data as string)
    } catch {
      return
    }
    const type = typeof payload.type === 'string' ? payload.type : ''
    if (type === 'progress') {
      const phase = typeof payload.phase === 'string' ? payload.phase : ''
      const raw = typeof payload.message === 'string' ? payload.message : null
      set({ analyzeMessage: raw })
      if (phase === 'done') set({ analyzePhase: 'running' })
    } else if (type === 'analysis') {
      const analysis = payload.analysis as Analysis
      const redacted = typeof payload.redacted_count === 'number' ? payload.redacted_count : null
      set({ analysis, analyzePhase: 'done', analyzeMessage: null, redactedCount: redacted })
      void refreshSensitive(recording, set)
    } else if (type === 'error') {
      const raw =
        typeof payload.message === 'string'
          ? payload.message
          : computerText('computerUse.msg.analysisFailed')
      set({ analyzePhase: 'error', analyzeError: localizeComputerMessage(null, raw) })
    }
  }
  ws.onclose = () => {
    if (analyzeSocket !== ws) return
    analyzeSocket = null
    if (get().analyzePhase === 'running') {
      set({
        analyzePhase: 'error',
        analyzeError: computerText('computerUse.msg.connectionClosed'),
      })
    }
  }
}

async function refreshSensitive(recording: string, set: (partial: Partial<Store>) => void) {
  try {
    const report = await getSensitiveReport(recording)
    set({ sensitiveReport: report })
  } catch {
  }
}

function runBuildSocket(
  recording: string,
  message: Record<string, unknown>,
  set: (partial: Partial<Store>) => void,
  get: () => Store,
  creating = false,
) {
  const { provider, model } = useComputerUseStore.getState()
  if (!provider || !model) {
    set({
      buildPhase: 'error',
      buildError: computerText('computerUse.msg.noVisionModel'),
      buildMessage: null,
    })
    return
  }
  closeSocket(buildSocket)
  set({
    buildPhase: creating ? 'creating' : 'planning',
    buildError: null,
    buildMessage: null,
    ...(creating ? {} : { builtPath: null, builtPlacement: null }),
  })
  let ws: WebSocket
  try {
    ws = new WebSocket(wsUrl('/ws/computer-build/session'))
  } catch (err) {
    set({
      buildPhase: 'error',
      buildError:
        err instanceof Error
          ? err.message
          : computerText('computerUse.msg.connectionOpenFailed'),
    })
    return
  }
  buildSocket = ws
  ws.onopen = () => {
    ws.send(JSON.stringify({ ...message, recording }))
  }
  ws.onmessage = (event) => {
    if (buildSocket !== ws) return
    let payload: Record<string, unknown>
    try {
      payload = JSON.parse(event.data as string)
    } catch {
      return
    }
    const type = typeof payload.type === 'string' ? payload.type : ''
    if (type === 'progress') {
      set({ buildMessage: typeof payload.message === 'string' ? payload.message : null })
    } else if (type === 'skill_plan') {
      set({ skillPlan: payload.plan as SkillPlan, buildPhase: 'plan', buildMessage: null })
    } else if (type === 'automation_plan') {
      set({
        automationPlan: payload.plan as AutomationPlan,
        buildPhase: 'plan',
        buildMessage: null,
      })
    } else if (type === 'built') {
      set({
        buildPhase: 'done',
        buildMessage: null,
        builtPath: typeof payload.path === 'string' ? payload.path : null,
        builtPlacement: typeof payload.placement === 'string' ? payload.placement : null,
      })
    } else if (type === 'error') {
      const raw =
        typeof payload.message === 'string'
          ? payload.message
          : computerText('computerUse.msg.buildFailed')
      set({ buildPhase: 'error', buildError: localizeComputerMessage(null, raw) })
    }
  }
  ws.onclose = () => {
    if (buildSocket !== ws) return
    buildSocket = null
    const phase = get().buildPhase
    if (phase === 'planning' || phase === 'creating') {
      set({
        buildPhase: 'error',
        buildError: computerText('computerUse.msg.connectionClosed'),
      })
    }
  }
}
