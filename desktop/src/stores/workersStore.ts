import { create } from 'zustand'
import { workersApi } from '../api/workers'
import type { WorkerSnapshot, WorkerStatus } from '../types/chat'

type WorkersByParent = Record<string, WorkerSnapshot[]>

type SpawnInput = {
  parentSessionId: string
  parentToolUseId: string
  workerId: string
  title: string
  model: string
}

type WorkersStore = {
  workersByParent: WorkersByParent
  workersById: Record<string, WorkerSnapshot>
  upsertWorker: (snapshot: WorkerSnapshot) => void
  spawnWorker: (input: SpawnInput) => void
  updateStatus: (
    workerId: string,
    status: WorkerStatus,
    detail?: string | null,
  ) => void
  updateProgress: (workerId: string, action: string, detail: string) => void
  markCompleted: (workerId: string, success: boolean, summary: string) => void
  markStopped: (workerId: string, reason: string) => void
  fetchByParent: (parentSessionId: string) => Promise<void>
  fetchAll: () => Promise<void>
  stopWorker: (workerId: string) => Promise<boolean>
  getById: (workerId: string) => WorkerSnapshot | undefined
  listByParent: (parentSessionId: string) => WorkerSnapshot[]
  listRunningByParent: (parentSessionId: string) => WorkerSnapshot[]
  hasRunningWorkers: (parentSessionId: string) => boolean
  resolveForSpawnCard: (
    toolUseId: string,
    parentSessionId: string,
    toolTimestamp?: number,
  ) => WorkerSnapshot[]
}

const SPAWN_WORKER_ASSOC_WINDOW_MS = 10 * 60 * 1000

export const EMPTY_WORKERS: WorkerSnapshot[] = []

function sortWorkers(list: WorkerSnapshot[]): WorkerSnapshot[] {
  return [...list].sort((a, b) => a.startedAt - b.startedAt)
}

function resolveWorkersForSpawnCard(
  workersById: Record<string, WorkerSnapshot>,
  toolUseId: string,
  parentSessionId: string,
  toolTimestamp?: number,
): WorkerSnapshot[] {
  const sessionWorkers = Object.values(workersById).filter(
    (w) => w.parentSessionId === parentSessionId,
  )
  const direct = sessionWorkers.filter((w) => w.parentToolUseId === toolUseId)
  if (direct.length > 0) return sortWorkers(direct)

  const orphans = sessionWorkers.filter((w) => !w.parentToolUseId?.trim())
  if (toolTimestamp != null && orphans.length > 0) {
    const windowed = orphans.filter(
      (w) =>
        w.startedAt >= toolTimestamp - 5_000 &&
        w.startedAt <= toolTimestamp + SPAWN_WORKER_ASSOC_WINDOW_MS,
    )
    if (windowed.length > 0) return sortWorkers(windowed)
  }

  if (orphans.length > 0) return sortWorkers(orphans)
  return sortWorkers(sessionWorkers)
}

function isTerminal(status: WorkerStatus): boolean {
  return status === 'completed' || status === 'failed' || status === 'stopped'
}

export const useWorkersStore = create<WorkersStore>((set, get) => ({
  workersByParent: {},
  workersById: {},

  upsertWorker: (snapshot) => {
    set((state) => {
      const existing = state.workersById[snapshot.workerId]
      const merged: WorkerSnapshot = existing
        ? { ...existing, ...snapshot }
        : snapshot
      const parent = merged.parentSessionId
      const list = state.workersByParent[parent] ?? []
      const without = list.filter((w) => w.workerId !== merged.workerId)
      return {
        workersById: { ...state.workersById, [merged.workerId]: merged },
        workersByParent: {
          ...state.workersByParent,
          [parent]: [...without, merged].sort((a, b) => a.startedAt - b.startedAt),
        },
      }
    })
  },

  spawnWorker: (input) => {
    const snapshot: WorkerSnapshot = {
      workerId: input.workerId,
      parentSessionId: input.parentSessionId,
      parentToolUseId: input.parentToolUseId,
      title: input.title,
      model: input.model,
      status: 'running',
      lastAction: null,
      lastDetail: null,
      startedAt: Date.now(),
      finishedAt: null,
    }
    get().upsertWorker(snapshot)
  },

  updateStatus: (workerId, status, detail) => {
    const existing = get().workersById[workerId]
    if (!existing) return
    const next: WorkerSnapshot = {
      ...existing,
      status,
      lastDetail: detail ?? existing.lastDetail ?? null,
      finishedAt: isTerminal(status) ? Date.now() : existing.finishedAt ?? null,
    }
    get().upsertWorker(next)
  },

  updateProgress: (workerId, action, detail) => {
    const existing = get().workersById[workerId]
    if (!existing) return
    get().upsertWorker({ ...existing, lastAction: action, lastDetail: detail })
  },

  markCompleted: (workerId, success, summary) => {
    const existing = get().workersById[workerId]
    if (!existing) return
    get().upsertWorker({
      ...existing,
      status: success ? 'completed' : 'failed',
      lastAction: success ? 'completed' : 'failed',
      lastDetail: summary,
      finishedAt: Date.now(),
    })
  },

  markStopped: (workerId, reason) => {
    const existing = get().workersById[workerId]
    if (!existing) return
    get().upsertWorker({
      ...existing,
      status: 'stopped',
      lastAction: 'stopped',
      lastDetail: reason,
      finishedAt: Date.now(),
    })
  },

  fetchByParent: async (parentSessionId) => {
    try {
      const list = await workersApi.list(parentSessionId)
      set((state) => {
        const merged = { ...state.workersById }
        list.forEach((w) => {
          merged[w.workerId] = w
        })
        return {
          workersById: merged,
          workersByParent: {
            ...state.workersByParent,
            [parentSessionId]: list,
          },
        }
      })
    } catch (err) {
      console.warn('workersStore.fetchByParent failed', err)
    }
  },

  fetchAll: async () => {
    try {
      const list = await workersApi.list()
      const byParent: WorkersByParent = {}
      const byId: Record<string, WorkerSnapshot> = {}
      list.forEach((w) => {
        byId[w.workerId] = w
        const bucket = byParent[w.parentSessionId] ?? []
        bucket.push(w)
        byParent[w.parentSessionId] = bucket
      })
      set({ workersById: byId, workersByParent: byParent })
    } catch (err) {
      console.warn('workersStore.fetchAll failed', err)
    }
  },

  stopWorker: async (workerId) => {
    try {
      const ok = await workersApi.cancel(workerId)
      if (ok) {
        const existing = get().workersById[workerId]
        if (existing && !isTerminal(existing.status)) {
          get().updateStatus(workerId, 'stopped', 'cancelled by user')
        }
      }
      return ok
    } catch (err) {
      console.warn('workersStore.stopWorker failed', err)
      return false
    }
  },

  getById: (workerId) => get().workersById[workerId],

  listByParent: (parentSessionId) =>
    get().workersByParent[parentSessionId] ?? EMPTY_WORKERS,

  listRunningByParent: (parentSessionId) =>
    (get().workersByParent[parentSessionId] ?? EMPTY_WORKERS).filter(
      (w) => !isTerminal(w.status),
    ),

  hasRunningWorkers: (parentSessionId) =>
    (get().workersByParent[parentSessionId] ?? EMPTY_WORKERS).some(
      (w) => !isTerminal(w.status),
    ),

  resolveForSpawnCard: (toolUseId, parentSessionId, toolTimestamp) =>
    resolveWorkersForSpawnCard(
      get().workersById,
      toolUseId,
      parentSessionId,
      toolTimestamp,
    ),
}))
