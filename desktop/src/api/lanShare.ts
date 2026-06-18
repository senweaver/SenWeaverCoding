// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { LanMyShare, LanPeerShare } from '../types/lanShare'

export const lanShareApi = {
  getMyShares() {
    return api.get<{ shares: LanMyShare[] }>('/api/lan/shares')
  },

  getPeerShares() {
    return api.get<{ shares: LanPeerShare[] }>('/api/lan/shares/peers')
  },

  addShare(path: string, note = '') {
    return api.post<{ ok: true; id: string }>('/api/lan/shares', { path, note })
  },

  removeShare(shareId: string) {
    return api.post<{ ok: true }>('/api/lan/shares/remove', { shareId })
  },

  download(ownerId: string, shareId: string) {
    return api.post<{ ok: true }>('/api/lan/shares/download', { ownerId, shareId })
  },
}
