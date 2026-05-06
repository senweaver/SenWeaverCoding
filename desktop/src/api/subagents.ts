import { api } from './client'
import type { DelegateAgentDef, DelegateAgentPatch } from '../types/subagents'

type RawAgent = Partial<DelegateAgentDef> & { name: string }

function normalise(raw: RawAgent): DelegateAgentDef {
  return {
    name: raw.name,
    provider: raw.provider ?? '',
    model: raw.model ?? '',
    systemPrompt: raw.systemPrompt ?? null,
    apiKey: raw.apiKey ?? null,
    temperature: raw.temperature ?? null,
    maxDepth: raw.maxDepth ?? 3,
    agentic: raw.agentic ?? false,
    allowedTools: raw.allowedTools ?? [],
    maxIterations: raw.maxIterations ?? 10,
    timeoutSecs: raw.timeoutSecs ?? null,
    agenticTimeoutSecs: raw.agenticTimeoutSecs ?? null,
    skillsDirectory: raw.skillsDirectory ?? null,
  }
}

export const subagentsApi = {
  get: async (name: string): Promise<DelegateAgentDef> => {
    const { agent } = await api.get<{ agent: RawAgent }>(
      `/api/agents/${encodeURIComponent(name)}`,
    )
    return normalise(agent)
  },
  create: async (def: DelegateAgentDef): Promise<DelegateAgentDef> => {
    const { agent } = await api.post<{ agent: RawAgent }>('/api/agents', def)
    return normalise(agent)
  },
  update: async (
    name: string,
    patch: DelegateAgentPatch,
  ): Promise<DelegateAgentDef> => {
    const { agent } = await api.put<{ agent: RawAgent }>(
      `/api/agents/${encodeURIComponent(name)}`,
      patch,
    )
    return normalise(agent)
  },
  remove: async (name: string): Promise<{ ok: boolean }> =>
    api.delete<{ ok: boolean }>(`/api/agents/${encodeURIComponent(name)}`),
}
