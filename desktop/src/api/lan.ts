// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api, getBaseUrl } from './client'
import type {
  LanConversation,
  LanIdentity,
  LanMessage,
  LanPeer,
  LanTransfer,
} from '../types/lan'

export const lanApi = {
  getIdentity() {
    return api.get<LanIdentity>('/api/lan/identity')
  },

  updateProfile(profile: { nickname?: string; email?: string | null }) {
    return api.put<LanIdentity>('/api/lan/profile', profile)
  },

  setDiscovery(enabled: boolean) {
    return api.post<{ ok: true; running: boolean }>('/api/lan/discovery', { enabled })
  },

  getPeers() {
    return api.get<{ peers: LanPeer[] }>('/api/lan/peers')
  },

  getConversations() {
    return api.get<{ conversations: LanConversation[]; unread: number }>(
      '/api/lan/conversations',
    )
  },

  getMessages(peerId: string, limit = 200) {
    return api.get<{ messages: LanMessage[] }>(
      `/api/lan/messages?peerId=${encodeURIComponent(peerId)}&limit=${limit}`,
    )
  },

  sendMessage(peerId: string, body: string) {
    return api.post<{ ok: true; id: string }>('/api/lan/messages', { peerId, body })
  },

  markRead(peerId: string) {
    return api.post<{ ok: true; unread: number }>('/api/lan/messages/read', { peerId })
  },

  sendFile(peerId: string, path: string) {
    return api.post<{ ok: true; transferId: string }>('/api/lan/files', { peerId, path })
  },

  sendImage(peerId: string, fileName: string, dataBase64: string) {
    return api.post<{ ok: true; transferId: string }>('/api/lan/files/image', {
      peerId,
      fileName,
      dataBase64,
    })
  },

  rawFileUrl(path: string) {
    return `${getBaseUrl()}/api/lan/files/raw?path=${encodeURIComponent(path)}`
  },

  saveFile(path: string, dest: string) {
    return api.post<{ ok: true; path: string }>('/api/lan/files/save', { path, dest })
  },

  getTransfers() {
    return api.get<{ transfers: LanTransfer[] }>('/api/lan/transfers')
  },
}

export function lanWebSocketUrl(): string {
  return `${getBaseUrl().replace(/^http/, 'ws')}/ws/lan`
}
