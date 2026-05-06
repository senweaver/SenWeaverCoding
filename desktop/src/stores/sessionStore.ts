import { create } from 'zustand'
import { sessionsApi } from '../api/sessions'
import { useSessionRuntimeStore } from './sessionRuntimeStore'
import { SCHEDULED_TAB_ID } from './tabStore'
import type { SessionListItem } from '../types/session'

const PINNED_SESSION_WORK_DIR_KEY = 'sen-user-pinned-session-work-dir'

function readPinnedWorkDir(): string | null {
  try {
    const raw = localStorage.getItem(PINNED_SESSION_WORK_DIR_KEY)?.trim()
    return raw && raw.length > 0 ? raw : null
  } catch {
    return null
  }
}

function persistPinnedWorkDir(dir: string | null) {
  try {
    if (dir?.trim()) {
      localStorage.setItem(PINNED_SESSION_WORK_DIR_KEY, dir.trim())
    } else {
      localStorage.removeItem(PINNED_SESSION_WORK_DIR_KEY)
    }
  } catch {

  }
}

type SessionStore = {
  sessions: SessionListItem[]
  activeSessionId: string | null
  isLoading: boolean
  error: string | null
  selectedProjects: string[]
  availableProjects: string[]

  userPinnedSessionWorkDir: string | null

  lastBrowsedSessionWorkDir: string | null

  fetchSessions: (project?: string) => Promise<void>
  createSession: (workDir?: string) => Promise<string>
  deleteSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  updateSessionTitle: (id: string, title: string) => void
  setActiveSession: (id: string | null) => void
  setSelectedProjects: (projects: string[]) => void
  setUserPinnedSessionWorkDir: (path: string | null | undefined) => void
  recordBrowseSessionWorkDir: (sessionId: string) => void
  resolveWorkDirForNewSessionTab: (activeTabId: string | null | undefined) => string | undefined
}

export const useSessionStore = create<SessionStore>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  isLoading: false,
  error: null,
  selectedProjects: [],
  availableProjects: [],
  userPinnedSessionWorkDir: readPinnedWorkDir(),
  lastBrowsedSessionWorkDir: null,

  fetchSessions: async (project?: string) => {
    set({ isLoading: true, error: null })
    try {
      const { sessions: raw } = await sessionsApi.list({ project, limit: 100 })

      const byId = new Map<string, SessionListItem>()
      for (const s of raw) {
        const existing = byId.get(s.id)
        if (!existing || new Date(s.modifiedAt) > new Date(existing.modifiedAt)) {
          byId.set(s.id, s)
        }
      }
      const sessions = [...byId.values()]
      const availableProjects = [...new Set(sessions.map((s) => s.projectPath).filter(Boolean))].sort()
      set({ sessions, availableProjects, isLoading: false })
    } catch (err) {
      set({ error: (err as Error).message, isLoading: false })
    }
  },

  createSession: async (workDir?: string) => {
    const { sessionId: id } = await sessionsApi.create(workDir || undefined)
    const now = new Date().toISOString()
    const optimisticSession: SessionListItem = {
      id,
      title: '',
      createdAt: now,
      modifiedAt: now,
      messageCount: 0,
      projectPath: workDir?.trim() ?? '',
      workDir: workDir?.trim() ? workDir.trim() : null,
      workDirExists: true,
    }

    set((state) => ({
      sessions: state.sessions.some((session) => session.id === id)
        ? state.sessions
        : [optimisticSession, ...state.sessions],
      activeSessionId: id,
    }))

    void get().fetchSessions()
    return id
  },

  deleteSession: async (id: string) => {
    await sessionsApi.delete(id)
    useSessionRuntimeStore.getState().clearSelection(id)
    set((s) => ({
      sessions: s.sessions.filter((session) => session.id !== id),
      activeSessionId: s.activeSessionId === id ? null : s.activeSessionId,
    }))
  },

  renameSession: async (id: string, title: string) => {
    await sessionsApi.rename(id, title)
    set((s) => ({
      sessions: s.sessions.map((session) =>
        session.id === id ? { ...session, title } : session,
      ),
    }))
  },

  updateSessionTitle: (id, title) => {
    set((s) => ({
      sessions: s.sessions.map((session) =>
        session.id === id ? { ...session, title } : session,
      ),
    }))
  },

  setActiveSession: (id) => set({ activeSessionId: id }),
  setSelectedProjects: (projects) => set({ selectedProjects: projects }),

  setUserPinnedSessionWorkDir: (path) => {
    const trimmed = path?.trim() ?? ''
    if (!trimmed) {
      persistPinnedWorkDir(null)
      set({ userPinnedSessionWorkDir: null })
      return
    }
    persistPinnedWorkDir(trimmed)
    set({ userPinnedSessionWorkDir: trimmed })
  },

  recordBrowseSessionWorkDir: (sessionId) => {
    if (!sessionId || sessionId.startsWith('__')) return
    const s = get().sessions.find((x) => x.id === sessionId)
    const wd = s?.workDir?.trim()
    if (wd) {
      set({ lastBrowsedSessionWorkDir: wd })
    }
  },

  resolveWorkDirForNewSessionTab: (activeTabId) => {
    const { sessions, userPinnedSessionWorkDir, lastBrowsedSessionWorkDir } = get()

    const onSessionChatTab = !!activeTabId && activeTabId !== SCHEDULED_TAB_ID

    if (onSessionChatTab) {
      const cur = sessions.find((x) => x.id === activeTabId)
      const wdCur = cur?.workDir?.trim()
      if (wdCur) {
        return wdCur
      }
    }

    const pinned = userPinnedSessionWorkDir?.trim()
    if (pinned) return pinned

    const last = lastBrowsedSessionWorkDir?.trim()
    if (last) return last

    return undefined
  },
}))
