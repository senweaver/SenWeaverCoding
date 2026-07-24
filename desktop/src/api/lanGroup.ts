// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api, getBaseUrl, withAuthToken } from './client'
import type {
  LanGroupMessage,
  LanGroupSnapshot,
  LanGroupSummary,
  LanGroupRole,
  TaskInputPayload,
} from '../types/lanGroup'

export const lanGroupApi = {
  list() {
    return api.get<{ groups: LanGroupSummary[]; unread: number }>('/api/lan/groups')
  },

  create(name: string, description: string) {
    return api.post<{ ok: true; group: LanGroupSummary }>('/api/lan/groups', {
      name,
      description,
    })
  },

  snapshot(groupId: string) {
    return api.get<LanGroupSnapshot>(
      `/api/lan/groups/snapshot?groupId=${encodeURIComponent(groupId)}`,
    )
  },

  messages(groupId: string, limit = 300) {
    return api.get<{ messages: LanGroupMessage[] }>(
      `/api/lan/groups/messages?groupId=${encodeURIComponent(groupId)}&limit=${limit}`,
    )
  },

  sendMessage(groupId: string, body: string) {
    return api.post<{ ok: true; id: string }>('/api/lan/groups/messages', { groupId, body })
  },

  markRead(groupId: string) {
    return api.post<{ ok: true; unread: number }>('/api/lan/groups/messages/read', { groupId })
  },

  updateMeta(groupId: string, name: string, description: string) {
    return api.post<{ ok: true }>('/api/lan/groups/meta', { groupId, name, description })
  },

  invite(groupId: string, userId: string, role: LanGroupRole) {
    return api.post<{ ok: true }>('/api/lan/groups/invite', { groupId, userId, role })
  },

  setRole(groupId: string, userId: string, role: LanGroupRole) {
    return api.post<{ ok: true }>('/api/lan/groups/members/role', { groupId, userId, role })
  },

  removeMember(groupId: string, userId: string) {
    return api.post<{ ok: true }>('/api/lan/groups/members/remove', { groupId, userId })
  },

  leave(groupId: string) {
    return api.post<{ ok: true }>('/api/lan/groups/leave', { groupId })
  },

  upsertPhase(input: {
    groupId: string
    phaseId?: string
    name: string
    order?: number
    status?: string
    color?: string
  }) {
    return api.post<{ ok: true }>('/api/lan/groups/phases', input)
  },

  removePhase(groupId: string, phaseId: string) {
    return api.post<{ ok: true }>('/api/lan/groups/phases/remove', { groupId, phaseId })
  },

  uploadDocument(groupId: string, path: string, phaseId: string, note: string) {
    return api.post<{ ok: true; docId: string }>('/api/lan/groups/documents', {
      groupId,
      path,
      phaseId,
      note,
    })
  },

  uploadImage(
    groupId: string,
    fileName: string,
    dataBase64: string,
    phaseId = '',
    note = '',
  ) {
    return api.post<{ ok: true; docId: string }>('/api/lan/groups/documents/image', {
      groupId,
      fileName,
      dataBase64,
      phaseId,
      note,
    })
  },

  rawDocumentUrl(groupId: string, docId: string) {
    return withAuthToken(
      `${getBaseUrl()}/api/lan/groups/documents/raw?groupId=${encodeURIComponent(
        groupId,
      )}&docId=${encodeURIComponent(docId)}`,
    )
  },

  downloadDocument(groupId: string, docId: string) {
    return api.post<{ ok: true; available: boolean }>('/api/lan/groups/documents/download', {
      groupId,
      docId,
    })
  },

  saveDocument(groupId: string, docId: string, dest: string) {
    return api.post<{ ok: true; path: string }>('/api/lan/groups/documents/save', {
      groupId,
      docId,
      dest,
    })
  },

  removeDocument(groupId: string, docId: string) {
    return api.post<{ ok: true }>('/api/lan/groups/documents/remove', { groupId, docId })
  },

  upsertTask(groupId: string, task: TaskInputPayload) {
    return api.post<{ ok: true; taskId: string }>('/api/lan/groups/tasks', { groupId, ...task })
  },

  removeTask(groupId: string, taskId: string) {
    return api.post<{ ok: true }>('/api/lan/groups/tasks/remove', { groupId, taskId })
  },
}
