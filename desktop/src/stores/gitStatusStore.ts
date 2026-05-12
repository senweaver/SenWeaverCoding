import { create } from 'zustand'
import { gitApi, type GitStatusEntry } from '../api/git'

export type GitStatusSeverity =
  | 'unmodified'
  | 'untracked'
  | 'ignored'
  | 'added'
  | 'renamed'
  | 'copied'
  | 'modified'
  | 'typeChanged'
  | 'deleted'
  | 'conflicted'

type RootStatus = {
  isRepo: boolean
  entries: Record<string, GitStatusEntry>
  dirAggregate: Record<string, GitStatusSeverity>
  fetchedAt: number
  loading: boolean
  error?: string
  etag?: string
}

type GitStatusState = {
  byRoot: Record<string, RootStatus>
  fetchStatus: (root: string, opts?: { forceRefresh?: boolean }) => Promise<void>
  scheduleRefresh: (root: string) => void
  clearRoot: (root: string) => void
}

const REFRESH_DEBOUNCE_MS = 800

const refreshTimers: Record<string, ReturnType<typeof setTimeout>> = {}

export const STATUS_SEVERITY_RANK: Record<GitStatusSeverity, number> = {
  unmodified: 0,
  ignored: 1,
  untracked: 2,
  added: 3,
  renamed: 4,
  copied: 4,
  modified: 5,
  typeChanged: 5,
  deleted: 6,
  conflicted: 7,
}

export function classifyEntry(entry: GitStatusEntry): GitStatusSeverity {
  const x = entry.index || ' '
  const y = entry.worktree || ' '
  if (x === 'U' || y === 'U' || (x === 'A' && y === 'A') || (x === 'D' && y === 'D')) {
    return 'conflicted'
  }
  if (x === '?' || y === '?') return 'untracked'
  if (x === '!' || y === '!') return 'ignored'
  if (x === 'D' || y === 'D') return 'deleted'
  if (x === 'R' || y === 'R') return 'renamed'
  if (x === 'C' || y === 'C') return 'copied'
  if (x === 'T' || y === 'T') return 'typeChanged'
  if (x === 'M' || y === 'M') return 'modified'
  if (x === 'A' || y === 'A') return 'added'
  return 'unmodified'
}

export function statusBadgeChar(severity: GitStatusSeverity): string {
  switch (severity) {
    case 'modified':
    case 'typeChanged':
      return 'M'
    case 'added':
      return 'A'
    case 'deleted':
      return 'D'
    case 'renamed':
      return 'R'
    case 'copied':
      return 'C'
    case 'conflicted':
      return 'U'
    case 'untracked':
      return '?'
    case 'ignored':
      return '!'
    case 'unmodified':
    default:
      return ''
  }
}

function buildDirAggregate(entries: Record<string, GitStatusEntry>): Record<string, GitStatusSeverity> {
  const agg: Record<string, GitStatusSeverity> = {}
  for (const entry of Object.values(entries)) {
    const severity = classifyEntry(entry)
    if (severity === 'unmodified') continue
    const segments = entry.relPath.split('/')
    let current = ''
    for (let i = 0; i < segments.length - 1; i += 1) {
      const segment = segments[i]
      if (!segment) continue
      current = current ? `${current}/${segment}` : segment
      const prev = agg[current]
      if (!prev || STATUS_SEVERITY_RANK[severity] > STATUS_SEVERITY_RANK[prev]) {
        agg[current] = severity
      }
    }
  }
  return agg
}

function indexEntries(entries: GitStatusEntry[]): Record<string, GitStatusEntry> {
  const map: Record<string, GitStatusEntry> = {}
  for (const e of entries) {
    map[e.relPath] = e
  }
  return map
}

export const useGitStatusStore = create<GitStatusState>((set, get) => ({
  byRoot: {},

  fetchStatus: async (root: string, opts) => {
    if (!root) return
    const force = opts?.forceRefresh === true
    const prevEtag = get().byRoot[root]?.etag
    set((s) => {
      const prev = s.byRoot[root]
      return {
        byRoot: {
          ...s.byRoot,
          [root]: {
            isRepo: prev?.isRepo ?? false,
            entries: prev?.entries ?? {},
            dirAggregate: prev?.dirAggregate ?? {},
            fetchedAt: prev?.fetchedAt ?? 0,
            loading: true,
            error: undefined,
            etag: prev?.etag,
          },
        },
      }
    })
    try {
      const result = await gitApi.fetchStatus({
        root,
        forceRefresh: force,
        etag: force ? undefined : prevEtag,
      })
      if (result.notModified) {
        set((s) => {
          const prev = s.byRoot[root]
          if (!prev) return {}
          return {
            byRoot: {
              ...s.byRoot,
              [root]: {
                ...prev,
                fetchedAt: Date.now(),
                loading: false,
                error: undefined,
                etag: result.etag || prev.etag,
              },
            },
          }
        })
        return
      }
      const entries = indexEntries(result.data.entries)
      const dirAggregate = buildDirAggregate(entries)
      set((s) => ({
        byRoot: {
          ...s.byRoot,
          [root]: {
            isRepo: result.data.isRepo,
            entries,
            dirAggregate,
            fetchedAt: result.data.computedAt,
            loading: false,
            error: undefined,
            etag: result.etag,
          },
        },
      }))
    } catch (err) {
      set((s) => {
        const prev = s.byRoot[root]
        return {
          byRoot: {
            ...s.byRoot,
            [root]: {
              isRepo: prev?.isRepo ?? false,
              entries: prev?.entries ?? {},
              dirAggregate: prev?.dirAggregate ?? {},
              fetchedAt: prev?.fetchedAt ?? 0,
              loading: false,
              error: err instanceof Error ? err.message : String(err),
              etag: prev?.etag,
            },
          },
        }
      })
    }
  },

  scheduleRefresh: (root: string) => {
    if (!root) return
    const existing = refreshTimers[root]
    if (existing) clearTimeout(existing)
    refreshTimers[root] = setTimeout(() => {
      delete refreshTimers[root]
      void get().fetchStatus(root, { forceRefresh: true })
    }, REFRESH_DEBOUNCE_MS)
  },

  clearRoot: (root: string) => {
    if (refreshTimers[root]) {
      clearTimeout(refreshTimers[root])
      delete refreshTimers[root]
    }
    set((s) => {
      if (!(root in s.byRoot)) return {}
      const next = { ...s.byRoot }
      delete next[root]
      return { byRoot: next }
    })
  },
}))

export function selectGitFileSeverity(root: string | null, relPath: string) {
  return (state: GitStatusState): GitStatusSeverity => {
    if (!root) return 'unmodified'
    const bucket = state.byRoot[root]
    if (!bucket) return 'unmodified'
    const entry = bucket.entries[relPath]
    if (!entry) return 'unmodified'
    return classifyEntry(entry)
  }
}

export function selectGitDirSeverity(root: string | null, relPath: string) {
  return (state: GitStatusState): GitStatusSeverity => {
    if (!root) return 'unmodified'
    const bucket = state.byRoot[root]
    if (!bucket) return 'unmodified'
    return bucket.dirAggregate[relPath] ?? 'unmodified'
  }
}

export function selectGitIsRepo(root: string | null) {
  return (state: GitStatusState): boolean => {
    if (!root) return false
    return state.byRoot[root]?.isRepo ?? false
  }
}
