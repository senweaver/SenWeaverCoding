// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { teamsApi } from '../api/teams'
import type { TeamSummary, TeamDetail, TeamMember, AgentColor } from '../types/team'
import { AGENT_COLORS } from '../types/team'
import type { TeamMemberStatus, UIMessage } from '../types/chat'
import { useChatStore, mapHistoryMessagesToUiMessages } from './chatStore'
import { useTabStore } from './tabStore'

const MEMBER_POLL_INTERVAL_MS = 1500

const memberSessionId = (agentId: string) => `team-member:${agentId}`

let memberPollTimer: ReturnType<typeof setInterval> | null = null
let polledMemberSessionId: string | null = null

function createMemberSessionState() {
  return {
    messages: [] as UIMessage[],
    chatState: 'idle' as const,
    connectionState: 'connected' as const,
    streamingText: '',
    streamingToolArgs: null,
    activeToolUseId: null,
    activeToolName: null,
    activeThinkingId: null,
    activeThinkingContent: '',
    activeThinkingStartedAt: null,
    activeThinkingLastChunkAt: null,
    pendingPermission: null,
    tokenUsage: { input_tokens: 0, output_tokens: 0 },
    cumulativeTokens: 0,
    cumulativeCostUsd: 0,
    elapsedSeconds: 0,
    statusVerb: '',
    slashCommands: [],
    agentTaskNotifications: {},
    pendingRewind: null,
    pendingSendAfterRewind: null,
    pendingEdits: [],
    subagentTimelines: {},
    activeTaskToolUseId: null,
    stopRequested: false,
    debugPiiStats: { total: 0, counts: {}, lastEventAt: null },
    pendingResourceWaits: [],
    providerRetry: null,
  }
}

function normalizeMemberStatus(status: string | undefined): TeamMember['status'] {
  if (status === 'running' || status === 'idle' || status === 'completed') {
    return status
  }
  return status === 'failed' ? 'error' : 'idle'
}

function toTeamMember(raw: Record<string, unknown>): TeamMember {
  return {
    agentId: (raw.agentId as string) || '',
    name: raw.name as string | undefined,
    role:
      (raw.name as string) ||
      (raw.agentType as string) ||
      (raw.role as string) ||
      (raw.agentId as string) ||
      '',
    status: normalizeMemberStatus(raw.status as string | undefined),
    currentTask: raw.currentTask as string | undefined,
    color: raw.color as AgentColor | undefined,
    sessionId: raw.sessionId as string | undefined,
  }
}

function isPendingMemberMessage(message: UIMessage): message is Extract<UIMessage, { type: 'user_text' }> & { pending: true } {
  return message.type === 'user_text' && message.pending === true
}

function memberMessageContentKey(m: UIMessage): string {
  const anyM = m as unknown as Record<string, unknown>
  const raw = anyM.content ?? anyM.delta ?? anyM.text ?? ''
  const text = typeof raw === 'string' ? raw : JSON.stringify(raw)
  const superseded = anyM.superseded ? '1' : '0'
  const pending = (anyM.pending ? '1' : '0')
  return `${m.type}|${superseded}|${pending}|${text}`
}

function sameMemberMessages(a: UIMessage[], b: UIMessage[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (memberMessageContentKey(a[i]!) !== memberMessageContentKey(b[i]!)) return false
  }
  return true
}

const MEMBER_PENDING_MESSAGE_MAX_AGE_MS = 5 * 60_000

function transcriptAlreadyContainsMessage(
  transcriptMessages: UIMessage[],
  pendingMessage: Extract<UIMessage, { type: 'user_text' }> & { pending: true },
): boolean {
  const pendingContent = pendingMessage.content.trim()
  return transcriptMessages.some((message) => (
    message.type === 'user_text' &&
    message.pending !== true &&
    (message.content === pendingMessage.content ||
      (pendingContent.length > 0 && message.content.includes(pendingContent)))
  ))
}

function mergeMemberTranscriptMessages(
  existingMessages: UIMessage[],
  transcriptMessages: UIMessage[],
): UIMessage[] {
  const now = Date.now()
  const pendingMessages = existingMessages
    .filter(isPendingMemberMessage)
    .filter(
      (message) => now - message.timestamp <= MEMBER_PENDING_MESSAGE_MAX_AGE_MS,
    )
    .filter(
      (message) => !transcriptAlreadyContainsMessage(transcriptMessages, message),
    )

  return pendingMessages.length > 0
    ? [...transcriptMessages, ...pendingMessages]
    : transcriptMessages
}

function syncMemberSessionMessages(
  sessionId: string,
  memberStatus: TeamMember['status'],
  messages: UIMessage[],
) {
  const hasPendingMessages = messages.some(isPendingMemberMessage)
  const nextChatState =
    memberStatus === 'running' || hasPendingMessages ? 'thinking' : 'idle'
  useChatStore.setState((state) => {
    const existing = state.sessions[sessionId]
    if (
      existing &&
      existing.messages === messages &&
      existing.chatState === nextChatState &&
      existing.connectionState === 'connected'
    ) {
      return {}
    }
    const nextState = existing ?? createMemberSessionState()
    return {
      sessions: {
        ...state.sessions,
        [sessionId]: {
          ...nextState,
          messages,
          connectionState: 'connected',
          chatState: nextChatState,
        },
      },
    }
  })
}

type TeamStore = {
  teams: TeamSummary[]
  activeTeam: TeamDetail | null
  memberColors: Map<string, AgentColor>
  error: string | null

  fetchTeams: () => Promise<void>
  fetchTeamDetail: (name: string) => Promise<void>
  getMemberBySessionId: (sessionId: string) => TeamMember | null
  refreshMemberSession: (sessionId: string) => Promise<void>
  openMemberSession: (member: TeamMember) => void
  sendMessageToMember: (sessionId: string, content: string) => Promise<void>
  startMemberPolling: (sessionId: string, force?: boolean) => void
  stopMemberPolling: () => void
  clearTeam: () => void

  handleTeamCreated: (teamName: string) => void
  handleTeamUpdate: (teamName: string, members: TeamMemberStatus[]) => void
  handleTeamDeleted: (teamName: string) => void
}

export const useTeamStore = create<TeamStore>((set, get) => ({
  teams: [],
  activeTeam: null,
  memberColors: new Map(),
  error: null,

  fetchTeams: async () => {
    set({ error: null })
    try {
      const { teams } = await teamsApi.list()
      set({ teams })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    }
  },

  fetchTeamDetail: async (name: string) => {
    set({ error: null })
    try {
      const raw = await teamsApi.get(name) as Record<string, unknown>
      const rawMembers = Array.isArray(raw.members) ? raw.members : []
      const members: TeamMember[] = rawMembers.map((m: Record<string, unknown>) => toTeamMember(m))
      const detail: TeamDetail = {
        name: raw.name as string,
        leadAgentId: raw.leadAgentId as string | undefined,
        leadSessionId: raw.leadSessionId as string | undefined,
        members,
        createdAt: raw.createdAt != null ? String(raw.createdAt) : undefined,
      }

      const colors = new Map<string, AgentColor>()
      detail.members.forEach((m, i) => {
        colors.set(m.agentId, AGENT_COLORS[i % AGENT_COLORS.length]!)
      })
      set({ activeTeam: detail, memberColors: colors })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    }
  },

  getMemberBySessionId: (sessionId: string) => {
    const team = get().activeTeam
    if (!team) return null
    return team.members.find(
      (m) => m.sessionId === sessionId || memberSessionId(m.agentId) === sessionId,
    ) ?? null
  },

  refreshMemberSession: async (sessionId) => {
    const team = get().activeTeam
    const member = get().getMemberBySessionId(sessionId)
    if (!team || !member) return

    try {
      const { messages } = await teamsApi.getMemberTranscript(team.name, member.agentId)
      const asEntries = messages.map((msg) => ({
        id: msg.id,
        type: msg.type,
        content: msg.content,
        timestamp: msg.timestamp,
        model: msg.model,
        parentToolUseId: msg.parentToolUseId,
      }))
      const transcriptMessages = mapHistoryMessagesToUiMessages(
        asEntries as Parameters<typeof mapHistoryMessagesToUiMessages>[0],
        { includeTeammateMessages: true },
      )
      const existingMessages = useChatStore.getState().sessions[sessionId]?.messages ?? []
      const mergedMessages = mergeMemberTranscriptMessages(
        existingMessages,
        transcriptMessages,
      )
      const finalMessages = sameMemberMessages(existingMessages, mergedMessages)
        ? existingMessages
        : mergedMessages
      syncMemberSessionMessages(sessionId, member.status, finalMessages)
    } catch {
      const existingMessages = useChatStore.getState().sessions[sessionId]?.messages ?? []
      syncMemberSessionMessages(sessionId, member.status, existingMessages)
    }
  },

  openMemberSession: (member: TeamMember) => {
    const team = get().activeTeam
    if (!team) return

    get().stopMemberPolling()

    const tabId = memberSessionId(member.agentId)
    useTabStore.getState().openTab(tabId, member.role, 'session')
    void get().refreshMemberSession(tabId)
    get().startMemberPolling(tabId)
  },

  sendMessageToMember: async (sessionId, content) => {
    const team = get().activeTeam
    const member = get().getMemberBySessionId(sessionId)
    if (!team || !member) {
      throw new Error('Team member session is no longer available')
    }

    await teamsApi.sendMemberMessage(team.name, member.agentId, content)
    get().startMemberPolling(sessionId, true)
    await get().refreshMemberSession(sessionId)
  },

  startMemberPolling: (sessionId, force = false) => {
    const member = get().getMemberBySessionId(sessionId)
    if (!member) return

    const hasPendingMessages =
      useChatStore.getState().sessions[sessionId]?.messages.some(isPendingMemberMessage) ?? false

    if (!force && polledMemberSessionId === sessionId && memberPollTimer) {
      return
    }

    if (member.status !== 'running' && !hasPendingMessages) {
      get().stopMemberPolling()
      return
    }

    get().stopMemberPolling()
    polledMemberSessionId = sessionId
    memberPollTimer = setInterval(() => {
      const currentTabId = useTabStore.getState().activeTabId
      if (currentTabId !== sessionId) {
        get().stopMemberPolling()
        return
      }
      void get().refreshMemberSession(sessionId)
    }, MEMBER_POLL_INTERVAL_MS)
  },

  stopMemberPolling: () => {
    if (memberPollTimer) {
      clearInterval(memberPollTimer)
      memberPollTimer = null
    }
    polledMemberSessionId = null
  },

  clearTeam: () => {
    get().stopMemberPolling()
    set({ activeTeam: null, memberColors: new Map() })
  },

  handleTeamCreated: (teamName: string) => {
    set((s) => ({
      teams: [...s.teams, { name: teamName, memberCount: 0 }],
    }))
    get().fetchTeamDetail(teamName)
    setTimeout(() => get().fetchTeamDetail(teamName), 1500)
    setTimeout(() => get().fetchTeamDetail(teamName), 4000)
    setTimeout(() => get().fetchTeamDetail(teamName), 8000)
  },

  handleTeamUpdate: (teamName: string, members: TeamMemberStatus[]) => {
    const team = get().activeTeam
    if (team && team.name === teamName) {
      if (members.length === 0) return

      if (members.length > team.members.length) {
        get().fetchTeamDetail(teamName)
      }

      const colors = get().memberColors
      const existingMap = new Map(team.members.map((m) => [m.agentId, m]))
      const incomingIds = new Set(members.map((m) => m.agentId))
      const kept = team.members.filter((m) => !incomingIds.has(m.agentId))
      const updatedMembers: TeamMember[] = [
        ...kept,
        ...members.map((m, i) => {
          const existing = existingMap.get(m.agentId)
          return {
            ...(existing ?? {}),
            name: existing?.name,
            agentId: m.agentId,
            role: m.role,
            status: normalizeMemberStatus(m.status),
            currentTask: m.currentTask,
            color: colors.get(m.agentId) ?? AGENT_COLORS[i % AGENT_COLORS.length]!,
            sessionId: existing?.sessionId,
          }
        }),
      ]
      set({ activeTeam: { ...team, members: updatedMembers } })

      const currentTabId = useTabStore.getState().activeTabId
      if (currentTabId) {
        const viewedMember = get().getMemberBySessionId(currentTabId)
        if (viewedMember) {
          void get().refreshMemberSession(currentTabId)
          get().startMemberPolling(currentTabId)
        }
      }
    }
  },

  handleTeamDeleted: (teamName: string) => {
    get().stopMemberPolling()
    set((s) => ({
      teams: s.teams.filter((t) => t.name !== teamName),
      activeTeam: s.activeTeam?.name === teamName ? null : s.activeTeam,
    }))
  },
}))
