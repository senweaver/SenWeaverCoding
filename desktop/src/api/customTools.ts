import { api } from './client'
import type { CustomToolDef, CustomToolPatch } from '../types/customTools'

export const customToolsApi = {
  list: () => api.get<{ tools: CustomToolDef[] }>('/api/custom-tools'),
  create: (payload: CustomToolDef) =>
    api.post<{ tool: CustomToolDef }>('/api/custom-tools', payload),
  update: (name: string, patch: CustomToolPatch) =>
    api.put<{ tool: CustomToolDef }>(
      `/api/custom-tools/${encodeURIComponent(name)}`,
      patch,
    ),
  remove: (name: string) =>
    api.delete<{ ok: boolean }>(`/api/custom-tools/${encodeURIComponent(name)}`),
}
