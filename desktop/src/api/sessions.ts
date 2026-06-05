// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { SessionListItem, MessageEntry, PendingRewindSummary } from '../types/session'

type SessionsResponse = { sessions: SessionListItem[]; total: number }
type MessagesResponse = { messages: MessageEntry[]; pendingRewind?: PendingRewindSummary | null }
type CreateSessionResponse = { sessionId: string }
export type SessionRewindResponse = {

  rewindId?: string
  target: {
    userMessageIndex: number
    userMessageCount: number
  }
  conversation: {
    messagesRemoved: number

    tombstonedCount?: number
    removedMessageIds?: string[]
  }
  code: {
    available: boolean
    reason?: string
    filesChanged: string[]
    insertions: number
    deletions: number
  }
}

export type RewindRestoreResponse = {
  ok: true
  rewindId: string
  restoredCount: number
  clearedTombstones: number
  filesChanged: string[]
}

export type RewindCommitResponse = {
  ok: true
  rewindId: string
  purgedCount: number
}

export type RecentProject = {
  projectPath: string
  realPath: string
  projectName: string
  isGit: boolean
  repoName: string | null
  branch: string | null
  modifiedAt: string
  sessionCount: number
}

export const sessionsApi = {
  list(params?: { project?: string; limit?: number; offset?: number }) {
    const query = new URLSearchParams()
    if (params?.project) query.set('project', params.project)
    if (params?.limit) query.set('limit', String(params.limit))
    if (params?.offset) query.set('offset', String(params.offset))
    const qs = query.toString()
    return api.get<SessionsResponse>(`/api/sessions${qs ? `?${qs}` : ''}`)
  },

  getMessages(sessionId: string) {
    return api.get<MessagesResponse>(`/api/sessions/${sessionId}/messages`)
  },

  create(workDir?: string) {
    return api.post<CreateSessionResponse>('/api/sessions', workDir ? { workDir } : {})
  },

  delete(sessionId: string) {
    return api.delete<{ ok: true }>(`/api/sessions/${sessionId}`)
  },

  deleteBatch(ids: string[]) {
    return api.post<{ ok: true; deleted: number }>('/api/sessions/delete-batch', { ids })
  },

  rename(sessionId: string, title: string) {
    return api.patch<{ ok: true }>(`/api/sessions/${sessionId}`, { title })
  },

  getRecentProjects(params?: { limit?: number; offset?: number }) {
    const query = new URLSearchParams()
    if (typeof params?.limit === 'number') query.set('limit', String(params.limit))
    if (typeof params?.offset === 'number') query.set('offset', String(params.offset))
    const qs = query.toString()
    return api.get<{ projects: RecentProject[]; total: number }>(
      `/api/sessions/recent-projects${qs ? `?${qs}` : ''}`,
    )
  },

  getGitInfo(sessionId: string) {
    return api.get<{ branch: string | null; repoName: string | null; workDir: string; changedFiles: number }>(`/api/sessions/${sessionId}/git-info`)
  },

  getSlashCommands(sessionId: string) {
    return api.get<{ commands: Array<{ name: string; description: string }> }>(`/api/sessions/${sessionId}/slash-commands`)
  },

  rewind(
    sessionId: string,
    body: {
      userMessageIndex: number
      dryRun?: boolean

      revertFiles?: boolean
    },
  ) {
    return api.post<SessionRewindResponse>(`/api/sessions/${sessionId}/rewind`, body, {
      timeout: 60_000,
    })
  },

  restoreRewind(sessionId: string, rewindId: string) {
    return api.post<RewindRestoreResponse>(`/api/sessions/${sessionId}/rewind/restore`, {
      rewindId,
    }, { timeout: 60_000 })
  },

  commitRewind(sessionId: string, rewindId: string) {
    return api.post<RewindCommitResponse>(`/api/sessions/${sessionId}/rewind/commit`, {
      rewindId,
    }, { timeout: 60_000 })
  },

  revertBatches(sessionId: string, editBatchIds: string[]) {
    return api.post<{
      ok: boolean
      revertedPaths: string[]
      failedBatchIds: string[]
    }>(
      `/api/sessions/${sessionId}/revert-batches`,
      { editBatchIds },
      { timeout: 60_000 },
    )
  },
}
