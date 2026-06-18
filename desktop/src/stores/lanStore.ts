// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { lanApi, lanWebSocketUrl } from '../api/lan'
import type {
  LanConversation,
  LanIdentity,
  LanMessage,
  LanPeer,
  LanTransfer,
} from '../types/lan'

type LanState = {
  identity: LanIdentity | null
  peers: LanPeer[]
  conversations: LanConversation[]
  messagesByPeer: Record<string, LanMessage[]>
  transfers: LanTransfer[]
  unread: number
  activePeerId: string | null
  panelOpen: boolean
  ready: boolean
  wsConnected: boolean
  error: string | null

  init: () => Promise<void>
  refreshIdentity: () => Promise<void>
  setDiscovery: (enabled: boolean) => Promise<void>
  updateProfile: (profile: { nickname?: string; email?: string | null }) => Promise<void>
  openPanel: () => void
  closePanel: () => void
  togglePanel: () => void
  selectPeer: (peerId: string) => Promise<void>
  sendMessage: (peerId: string, body: string) => Promise<void>
  sendFile: (peerId: string, path: string) => Promise<void>
  sendImage: (peerId: string, fileName: string, dataBase64: string) => Promise<void>
  saveReceivedFile: (path: string, dest: string) => Promise<string>
  markRead: (peerId: string) => Promise<void>
}

let socket: WebSocket | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let reconnectAttempt = 0
let started = false

function scheduleReconnect(connect: () => void) {
  if (reconnectTimer) return
  const delay = Math.min(1000 * 2 ** reconnectAttempt, 15_000)
  reconnectAttempt += 1
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null
    connect()
  }, delay)
}

export const useLanStore = create<LanState>((set, get) => {
  function applyMessage(message: LanMessage) {
    set((state) => {
      const existing = state.messagesByPeer[message.peerId] ?? []
      if (existing.some((m) => m.id === message.id)) {
        return state
      }
      const merged = [...existing, message].sort((a, b) => a.createdAt - b.createdAt)
      const next: Partial<LanState> = {
        messagesByPeer: { ...state.messagesByPeer, [message.peerId]: merged },
      }
      return next as LanState
    })
    void refreshConversations()
    if (message.direction === 'in' && get().activePeerId === message.peerId && get().panelOpen) {
      void get().markRead(message.peerId)
    }
  }

  async function refreshConversations() {
    try {
      const res = await lanApi.getConversations()
      set({ conversations: res.conversations, unread: res.unread })
    } catch {
      // ignore transient errors
    }
  }

  function handleEvent(raw: string) {
    let parsed: { type?: string; kind?: string; data?: unknown }
    try {
      parsed = JSON.parse(raw)
    } catch {
      return
    }
    if (parsed.type !== 'lan_event') return
    const data = parsed.data as Record<string, unknown>
    switch (parsed.kind) {
      case 'lan_status': {
        const running = Boolean((data as { running?: boolean }).running)
        const port = Number((data as { port?: number }).port ?? 0)
        set((state) =>
          state.identity ? { identity: { ...state.identity, running, port } } : state,
        )
        break
      }
      case 'lan_identity': {
        set({ identity: data as unknown as LanIdentity })
        break
      }
      case 'lan_peers': {
        set({ peers: ((data as { peers?: LanPeer[] }).peers ?? []) })
        break
      }
      case 'lan_message': {
        const message = (data as { message?: LanMessage }).message
        if (message) applyMessage(message)
        break
      }
      case 'lan_unread': {
        set({ unread: Number((data as { unread?: number }).unread ?? 0) })
        break
      }
      case 'lan_transfer': {
        const transfer = (data as { transfer?: LanTransfer }).transfer
        if (transfer) {
          set((state) => {
            const others = state.transfers.filter((t) => t.id !== transfer.id)
            return { transfers: [transfer, ...others].slice(0, 200) }
          })
        }
        break
      }
      default:
        break
    }
  }

  function connect() {
    if (typeof WebSocket === 'undefined') return
    try {
      socket = new WebSocket(lanWebSocketUrl())
    } catch {
      scheduleReconnect(connect)
      return
    }
    socket.onopen = () => {
      reconnectAttempt = 0
      set({ wsConnected: true })
    }
    socket.onmessage = (event) => {
      handleEvent(event.data as string)
    }
    socket.onclose = () => {
      set({ wsConnected: false })
      socket = null
      scheduleReconnect(connect)
    }
    socket.onerror = () => {
      try {
        socket?.close()
      } catch {
        // ignore
      }
    }
  }

  return {
    identity: null,
    peers: [],
    conversations: [],
    messagesByPeer: {},
    transfers: [],
    unread: 0,
    activePeerId: null,
    panelOpen: false,
    ready: false,
    wsConnected: false,
    error: null,

    async init() {
      if (started) return
      started = true
      try {
        const [identity, peersRes, conversationsRes, transfersRes] = await Promise.all([
          lanApi.getIdentity(),
          lanApi.getPeers(),
          lanApi.getConversations(),
          lanApi.getTransfers(),
        ])
        set({
          identity,
          peers: peersRes.peers,
          conversations: conversationsRes.conversations,
          unread: conversationsRes.unread,
          transfers: transfersRes.transfers,
          ready: true,
          error: null,
        })
      } catch (err) {
        set({ ready: true, error: err instanceof Error ? err.message : 'lan init failed' })
      }
      connect()
    },

    async refreshIdentity() {
      try {
        const identity = await lanApi.getIdentity()
        set({ identity })
      } catch {
        // ignore
      }
    },

    async setDiscovery(enabled: boolean) {
      await lanApi.setDiscovery(enabled)
      await get().refreshIdentity()
      if (enabled) {
        try {
          const peersRes = await lanApi.getPeers()
          set({ peers: peersRes.peers })
        } catch {
          // ignore
        }
      } else {
        set({ peers: [] })
      }
    },

    async updateProfile(profile) {
      const identity = await lanApi.updateProfile(profile)
      set({ identity })
    },

    openPanel() {
      set({ panelOpen: true })
    },
    closePanel() {
      set({ panelOpen: false })
    },
    togglePanel() {
      set((state) => ({ panelOpen: !state.panelOpen }))
    },

    async selectPeer(peerId: string) {
      set({ activePeerId: peerId })
      try {
        const res = await lanApi.getMessages(peerId)
        set((state) => ({
          messagesByPeer: { ...state.messagesByPeer, [peerId]: res.messages },
        }))
      } catch {
        // ignore
      }
      await get().markRead(peerId)
    },

    async sendMessage(peerId: string, body: string) {
      const trimmed = body.trim()
      if (!trimmed) return
      await lanApi.sendMessage(peerId, trimmed)
    },

    async sendFile(peerId: string, path: string) {
      await lanApi.sendFile(peerId, path)
    },

    async sendImage(peerId: string, fileName: string, dataBase64: string) {
      if (!peerId || !dataBase64) return
      await lanApi.sendImage(peerId, fileName, dataBase64)
    },

    async saveReceivedFile(path: string, dest: string) {
      const res = await lanApi.saveFile(path, dest)
      return res.path
    },

    async markRead(peerId: string) {
      try {
        const res = await lanApi.markRead(peerId)
        set((state) => {
          const list = state.messagesByPeer[peerId]
          const messagesByPeer = list
            ? {
                ...state.messagesByPeer,
                [peerId]: list.map((m) =>
                  m.direction === 'in' ? { ...m, read: true } : m,
                ),
              }
            : state.messagesByPeer
          return {
            unread: res.unread,
            messagesByPeer,
            conversations: state.conversations.map((c) =>
              c.peerId === peerId ? { ...c, unread: 0 } : c,
            ),
          }
        })
      } catch {
        // ignore
      }
    },
  }
})
