import { create } from 'zustand'
import { subagentsApi } from '../api/subagents'
import { agentsApi } from '../api/agents'
import type { DelegateAgentDef, DelegateAgentPatch } from '../types/subagents'

type SubagentsStore = {
  agents: DelegateAgentDef[]
  isLoading: boolean
  isSaving: boolean
  error: string | null

  fetch: () => Promise<void>
  create: (def: DelegateAgentDef) => Promise<DelegateAgentDef>
  update: (name: string, patch: DelegateAgentPatch) => Promise<DelegateAgentDef>
  remove: (name: string) => Promise<void>
}

function rawToDef(raw: {
  agentType: string
  systemPrompt?: string
  model?: string
  tools?: string[]
}): DelegateAgentDef {

  const modelDisplay = raw.model ?? ''
  const [provider, ...modelParts] = modelDisplay.includes('/')
    ? modelDisplay.split('/')
    : ['', modelDisplay]
  return {
    name: raw.agentType,
    provider: provider || '',
    model: modelParts.join('/') || raw.model || '',
    systemPrompt: raw.systemPrompt ?? null,
    apiKey: null,
    temperature: null,
    maxDepth: 3,
    agentic: false,
    allowedTools: raw.tools ?? [],
    maxIterations: 10,
    timeoutSecs: null,
    agenticTimeoutSecs: null,
    skillsDirectory: null,
  }
}

export const useSubagentsStore = create<SubagentsStore>((set, get) => ({
  agents: [],
  isLoading: false,
  isSaving: false,
  error: null,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const { allAgents } = await agentsApi.list()

      const agents = await Promise.all(
        allAgents.map(async (a) => {
          try {
            return await subagentsApi.get(a.agentType)
          } catch {
            return rawToDef(a)
          }
        }),
      )
      agents.sort((a, b) => a.name.localeCompare(b.name))
      set({ agents, isLoading: false })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  create: async (def) => {
    set({ isSaving: true, error: null })
    try {
      const created = await subagentsApi.create(def)
      const next = [...get().agents, created].sort((a, b) =>
        a.name.localeCompare(b.name),
      )
      set({ agents: next, isSaving: false })
      return created
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },

  update: async (name, patch) => {
    set({ isSaving: true, error: null })
    try {
      const updated = await subagentsApi.update(name, patch)
      set({
        agents: get().agents.map((a) => (a.name === name ? updated : a)),
        isSaving: false,
      })
      return updated
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },

  remove: async (name) => {
    set({ isSaving: true, error: null })
    try {
      await subagentsApi.remove(name)
      set({
        agents: get().agents.filter((a) => a.name !== name),
        isSaving: false,
      })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },
}))
