// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { lanWebSocketUrl } from '../api/lan'
import { lanGroupApi } from '../api/lanGroup'
import type {
  LanGroupMessage,
  LanGroupSnapshot,
  LanGroupSummary,
  LanGroupRole,
  TaskInputPayload,
} from '../types/lanGroup'

export type GroupTab = 'chat' | 'documents' | 'board' | 'timeline' | 'members'

type LanGroupState = {
  groups: LanGroupSummary[]
  unread: number
  activeGroupId: string | null
  activeTab: GroupTab
  snapshots: Record<string, LanGroupSnapshot>
  messagesByGroup: Record<string, LanGroupMessage[]>
  panelOpen: boolean
  ready: boolean
  pendingUploadPath: string | null
  error: string | null

  init: () => Promise<void>
  refreshGroups: () => Promise<void>
  openPanel: () => void
  closePanel: () => void
  togglePanel: () => void
  setActiveTab: (tab: GroupTab) => void
  selectGroup: (groupId: string | null) => Promise<void>
  refreshSnapshot: (groupId: string) => Promise<void>
  createGroup: (name: string, description: string) => Promise<LanGroupSummary | null>
  updateMeta: (groupId: string, name: string, description: string) => Promise<void>
  invite: (groupId: string, userId: string, role: LanGroupRole) => Promise<void>
  setRole: (groupId: string, userId: string, role: LanGroupRole) => Promise<void>
  removeMember: (groupId: string, userId: string) => Promise<void>
  leaveGroup: (groupId: string) => Promise<void>
  upsertPhase: (input: {
    groupId: string
    phaseId?: string
    name: string
    order?: number
    status?: string
    color?: string
  }) => Promise<void>
  removePhase: (groupId: string, phaseId: string) => Promise<void>
  uploadDocument: (groupId: string, path: string, phaseId: string, note: string) => Promise<void>
  downloadDocument: (groupId: string, docId: string) => Promise<boolean>
  saveDocument: (groupId: string, docId: string, dest: string) => Promise<string>
  removeDocument: (groupId: string, docId: string) => Promise<void>
  upsertTask: (groupId: string, task: TaskInputPayload) => Promise<void>
  removeTask: (groupId: string, taskId: string) => Promise<void>
  sendMessage: (groupId: string, body: string) => Promise<void>
  sendImage: (groupId: string, fileName: string, dataBase64: string) => Promise<void>
  markRead: (groupId: string) => Promise<void>
  stageUpload: (path: string) => void
  clearPendingUpload: () => void
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

export const useLanGroupStore = create<LanGroupState>((set, get) => {
  function applyMessage(groupId: string, message: LanGroupMessage) {
    set((state) => {
      const existing = state.messagesByGroup[groupId] ?? []
      if (existing.some((m) => m.id === message.id)) {
        return state
      }
      const merged = [...existing, message].sort((a, b) => a.tsMs - b.tsMs)
      return {
        messagesByGroup: { ...state.messagesByGroup, [groupId]: merged },
      }
    })
    const { activeGroupId, panelOpen } = get()
    if (activeGroupId === groupId && panelOpen) {
      void get().markRead(groupId)
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
    const data = (parsed.data ?? {}) as Record<string, unknown>
    switch (parsed.kind) {
      case 'lan_groups': {
        const groups = (data.groups as LanGroupSummary[]) ?? []
        const next: Partial<LanGroupState> = { groups }
        if (typeof data.unread === 'number') next.unread = data.unread as number
        set(next as LanGroupState)
        break
      }
      case 'lan_group_changed': {
        const groupId = String(data.groupId ?? '')
        if (groupId && get().activeGroupId === groupId) {
          void get().refreshSnapshot(groupId)
        }
        void get().refreshGroups()
        break
      }
      case 'lan_group_message': {
        const groupId = String(data.groupId ?? '')
        const message = data.message as LanGroupMessage | undefined
        if (groupId && message) applyMessage(groupId, message)
        break
      }
      case 'lan_group_unread': {
        set({ unread: Number(data.unread ?? 0) })
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
        // ignore
      }
    }
  }

  return {
    groups: [],
    unread: 0,
    activeGroupId: null,
    activeTab: 'chat',
    snapshots: {},
    messagesByGroup: {},
    panelOpen: false,
    ready: false,
    pendingUploadPath: null,
    error: null,

    async init() {
      if (started) return
      started = true
      try {
        const res = await lanGroupApi.list()
        set({ groups: res.groups, unread: res.unread, ready: true, error: null })
      } catch (err) {
        set({ ready: true, error: err instanceof Error ? err.message : 'lan group init failed' })
      }
      connect()
    },

    async refreshGroups() {
      try {
        const res = await lanGroupApi.list()
        set({ groups: res.groups, unread: res.unread })
      } catch {
        // ignore transient errors
      }
    },

    openPanel() {
      set({ panelOpen: true })
      void get().refreshGroups()
    },
    closePanel() {
      set({ panelOpen: false })
    },
    togglePanel() {
      const open = !get().panelOpen
      set({ panelOpen: open })
      if (open) void get().refreshGroups()
    },
    setActiveTab(tab) {
      set({ activeTab: tab })
    },

    async selectGroup(groupId) {
      set({ activeGroupId: groupId })
      if (!groupId) return
      await get().refreshSnapshot(groupId)
      try {
        const res = await lanGroupApi.messages(groupId)
        set((state) => ({
          messagesByGroup: { ...state.messagesByGroup, [groupId]: res.messages },
        }))
      } catch {
        // ignore
      }
      await get().markRead(groupId)
    },

    async refreshSnapshot(groupId) {
      try {
        const snapshot = await lanGroupApi.snapshot(groupId)
        set((state) => ({
          snapshots: { ...state.snapshots, [groupId]: snapshot },
        }))
      } catch {
        // ignore
      }
    },

    async createGroup(name, description) {
      const res = await lanGroupApi.create(name, description)
      await get().refreshGroups()
      return res.group
    },

    async updateMeta(groupId, name, description) {
      await lanGroupApi.updateMeta(groupId, name, description)
      await get().refreshSnapshot(groupId)
    },

    async invite(groupId, userId, role) {
      await lanGroupApi.invite(groupId, userId, role)
      await get().refreshSnapshot(groupId)
    },

    async setRole(groupId, userId, role) {
      await lanGroupApi.setRole(groupId, userId, role)
      await get().refreshSnapshot(groupId)
    },

    async removeMember(groupId, userId) {
      await lanGroupApi.removeMember(groupId, userId)
      await get().refreshSnapshot(groupId)
    },

    async leaveGroup(groupId) {
      await lanGroupApi.leave(groupId)
      set((state) => ({
        activeGroupId: state.activeGroupId === groupId ? null : state.activeGroupId,
      }))
      await get().refreshGroups()
    },

    async upsertPhase(input) {
      await lanGroupApi.upsertPhase(input)
      await get().refreshSnapshot(input.groupId)
    },

    async removePhase(groupId, phaseId) {
      await lanGroupApi.removePhase(groupId, phaseId)
      await get().refreshSnapshot(groupId)
    },

    async uploadDocument(groupId, path, phaseId, note) {
      await lanGroupApi.uploadDocument(groupId, path, phaseId, note)
      await get().refreshSnapshot(groupId)
    },

    async downloadDocument(groupId, docId) {
      const res = await lanGroupApi.downloadDocument(groupId, docId)
      return res.available
    },

    async saveDocument(groupId, docId, dest) {
      const res = await lanGroupApi.saveDocument(groupId, docId, dest)
      return res.path
    },

    async removeDocument(groupId, docId) {
      await lanGroupApi.removeDocument(groupId, docId)
      await get().refreshSnapshot(groupId)
    },

    async upsertTask(groupId, task) {
      await lanGroupApi.upsertTask(groupId, task)
      await get().refreshSnapshot(groupId)
    },

    async removeTask(groupId, taskId) {
      await lanGroupApi.removeTask(groupId, taskId)
      await get().refreshSnapshot(groupId)
    },

    async sendMessage(groupId, body) {
      const trimmed = body.trim()
      if (!trimmed) return
      await lanGroupApi.sendMessage(groupId, trimmed)
    },

    async sendImage(groupId, fileName, dataBase64) {
      if (!groupId || !dataBase64) return
      await lanGroupApi.uploadImage(groupId, fileName, dataBase64)
      await get().refreshSnapshot(groupId)
    },

    async markRead(groupId) {
      try {
        const res = await lanGroupApi.markRead(groupId)
        set((state) => ({
          unread: res.unread,
          groups: state.groups.map((g) => (g.id === groupId ? { ...g, unread: 0 } : g)),
        }))
      } catch {
        // ignore
      }
    },

    stageUpload(path) {
      set({ pendingUploadPath: path, panelOpen: true })
      void get().refreshGroups()
    },
    clearPendingUpload() {
      set({ pendingUploadPath: null })
    },
  }
})
