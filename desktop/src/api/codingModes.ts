import { api } from './client'
import type { CodingModeId, CodingModeInfo } from '../types/codingMode'

export const codingModesApi = {

  list() {
    return api.get<{ modes: CodingModeInfo[] }>('/api/coding-modes')
  },

  getCurrent() {
    return api.get<CodingModeInfo & { mode: CodingModeId }>('/api/coding-mode')
  },

  setCurrent(mode: CodingModeId) {
    return api.put<{ ok: true; mode: CodingModeId; permissionMode: string }>(
      '/api/coding-mode',
      { mode },
    )
  },
}
