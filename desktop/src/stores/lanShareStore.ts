// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { lanWebSocketUrl } from '../api/lan'
import { lanShareApi } from '../api/lanShare'
import type { LanMyShare, LanPeerShare, LanShareDownloaded } from '../types/lanShare'

type LanShareState = {
  myShares: LanMyShare[]
  peerShares: LanPeerShare[]
  downloads: LanShareDownloaded[]
  panelOpen: boolean
  ready: boolean
  error: string | null

  init: () => Promise<void>
  refresh: () => Promise<void>
  openPanel: () => void
  closePanel: () => void
  togglePanel: () => void
  addShare: (path: string, note?: string) => Promise<void>
  removeShare: (shareId: string) => Promise<void>
  download: (ownerId: string, shareId: string) => Promise<void>
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

export const useLanShareStore = create<LanShareState>((set, get) => {
  function handleEvent(raw: string) {
    let parsed: { type?: string; kind?: string; data?: unknown }
    try {
      parsed = JSON.parse(raw)
    } catch {
      return
    }
    if (parsed.type !== 'lan_event') return
    const data = (parsed.data ?? {}) as Record<string, unknown>
    switch (parsed.kind) {
      case 'lan_shares': {
        set({ myShares: (data.shares as LanMyShare[]) ?? [] })
        break
      }
      case 'lan_share_peers': {
        set({ peerShares: (data.shares as LanPeerShare[]) ?? [] })
        break
      }
      case 'lan_share_downloaded': {
        const entry = data as unknown as LanShareDownloaded
        set((state) => ({ downloads: [entry, ...state.downloads].slice(0, 50) }))
        break
      }
      case 'lan_peers': {
        void get().refresh()
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
    }
    socket.onmessage = (event) => {
      handleEvent(event.data as string)
    }
    socket.onclose = () => {
      socket = null
      scheduleReconnect(connect)
    }
    socket.onerror = () => {
      try {
        socket?.close()
      } catch {
      }
    }
  }

  return {
    myShares: [],
    peerShares: [],
    downloads: [],
    panelOpen: false,
    ready: false,
    error: null,

    async init() {
      if (started) return
      started = true
      await get().refresh()
      set({ ready: true })
      connect()
    },

    async refresh() {
      try {
        const [mine, peers] = await Promise.all([
          lanShareApi.getMyShares(),
          lanShareApi.getPeerShares(),
        ])
        set({ myShares: mine.shares, peerShares: peers.shares, error: null })
      } catch (err) {
        set({ error: err instanceof Error ? err.message : 'lan share refresh failed' })
      }
    },

    openPanel() {
      set({ panelOpen: true })
      void get().refresh()
    },
    closePanel() {
      set({ panelOpen: false })
    },
    togglePanel() {
      const open = !get().panelOpen
      set({ panelOpen: open })
      if (open) void get().refresh()
    },

    async addShare(path, note = '') {
      await lanShareApi.addShare(path, note)
      await get().refresh()
    },

    async removeShare(shareId) {
      await lanShareApi.removeShare(shareId)
      await get().refresh()
    },

    async download(ownerId, shareId) {
      await lanShareApi.download(ownerId, shareId)
    },
  }
})
