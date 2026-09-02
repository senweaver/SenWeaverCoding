// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import { getAuthToken, getBaseUrl, withAuthToken } from './client'
import type {
  FileContent,
  FileSearchResponse,
  FileTreeResponse,
  WriteFileResponse,
} from '../types/workspaceFile'

export type WorkspaceWatchEvent = {
  kind: 'created' | 'modified' | 'removed' | 'renamed' | 'resync'
  relPath: string
  fromRelPath?: string
}

export type WorkspaceCopyEvent =
  | {
      type: 'progress'
      bytesDone: number
      bytesTotal: number
      filesDone: number
      filesTotal: number
      currentRelPath: string
    }
  | { type: 'done'; toPath: string }
  | { type: 'error'; message: string }

function qs(params: Record<string, string | number | boolean | undefined>) {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined) continue
    search.set(key, String(value))
  }
  const out = search.toString()
  return out ? `?${out}` : ''
}

export const workspaceFilesApi = {
  tree(opts: {
    root: string
    path?: string
    depth?: number
    showHidden?: boolean
  }) {
    return api.get<FileTreeResponse>(
      `/api/workspace/tree${qs({
        root: opts.root,
        path: opts.path,
        depth: opts.depth,
        showHidden: opts.showHidden,
      })}`,
    )
  },

  readFile(opts: { root: string; path: string }) {
    return api.get<FileContent>(
      `/api/workspace/file${qs({ root: opts.root, path: opts.path })}`,
    )
  },

  rawHandle(opts: { root: string }) {
    return api.get<{ rawId: string }>(
      `/api/workspace/raw-handle${qs({ root: opts.root })}`,
    )
  },

  rawUrl(rawId: string, relPath: string, version?: number): string {
    const segs = relPath
      .split('/')
      .filter(Boolean)
      .map((s) => encodeURIComponent(s))
      .join('/')
    const trail = segs.length && relPath.endsWith('/') ? '/' : ''
    let url = withAuthToken(
      `${getBaseUrl()}/api/workspace/raw/${encodeURIComponent(rawId)}/${segs}${trail}`,
    )
    if (version) {
      url += `${url.includes('?') ? '&' : '?'}v=${version}`
    }
    return url
  },

  writeFile(opts: {
    root: string
    path: string
    content: string
    ifMatchMtime?: string
    encoding?: 'utf8' | 'base64'
  }) {
    return api.put<WriteFileResponse>('/api/workspace/file', {
      root: opts.root,
      path: opts.path,
      content: opts.content,
      ifMatchMtime: opts.ifMatchMtime,
      encoding: opts.encoding ?? 'utf8',
    })
  },

  createFile(opts: {
    root: string
    path: string
    content?: string
    encoding?: 'utf8' | 'base64'
  }) {
    return api.post<WriteFileResponse>('/api/workspace/file', {
      root: opts.root,
      path: opts.path,
      content: opts.content ?? '',
      encoding: opts.encoding ?? 'utf8',
    })
  },

  createDir(opts: { root: string; path: string }) {
    return api.post<{ ok: boolean; relPath: string }>('/api/workspace/dir', {
      root: opts.root,
      path: opts.path,
    })
  },

  move(opts: { root: string; fromPath: string; toPath: string }) {
    return api.post<{ ok: boolean; fromPath: string; toPath: string }>(
      '/api/workspace/move',
      opts,
    )
  },

  async copyStream(
    opts: { root: string; fromPath: string; toDir: string; signal?: AbortSignal },
    onEvent: (event: WorkspaceCopyEvent) => void,
  ): Promise<void> {
    const copyHeaders: Record<string, string> = { 'Content-Type': 'application/json' }
    const copyToken = getAuthToken()
    if (copyToken) {
      copyHeaders['X-Sen-Gateway-Token'] = copyToken
    }
    const res = await fetch(`${getBaseUrl()}/api/workspace/copy`, {
      method: 'POST',
      headers: copyHeaders,
      body: JSON.stringify({
        root: opts.root,
        fromPath: opts.fromPath,
        toDir: opts.toDir,
      }),
      signal: opts.signal,
    })
    if (!res.ok) {
      let message = `Copy failed (${res.status})`
      try {
        const body = (await res.json()) as { error?: string }
        if (body && typeof body.error === 'string' && body.error.length > 0) {
          message = body.error
        }
      } catch {
      }
      throw new Error(message)
    }
    if (!res.body) {
      throw new Error('Copy response has no body')
    }
    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''
    const flush = (chunk: string) => {
      buffer += chunk
      let sep = buffer.indexOf('\n\n')
      while (sep !== -1) {
        const block = buffer.slice(0, sep)
        buffer = buffer.slice(sep + 2)
        for (const line of block.split('\n')) {
          const trimmed = line.trimStart()
          if (!trimmed.startsWith('data:')) continue
          const payload = trimmed.slice(5).trim()
          if (!payload || payload === 'keep-alive') continue
          try {
            onEvent(JSON.parse(payload) as WorkspaceCopyEvent)
          } catch {
          }
        }
        sep = buffer.indexOf('\n\n')
      }
    }
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      flush(decoder.decode(value, { stream: true }))
    }
    flush(decoder.decode())
  },

  remove(opts: { root: string; path: string; recursive?: boolean }) {
    return api.delete<{ ok: boolean }>(
      `/api/workspace/entry${qs({
        root: opts.root,
        path: opts.path,
        recursive: opts.recursive,
      })}`,
    )
  },

  upload(opts: {
    root: string
    path: string
    contentBase64: string
    overwrite?: boolean
  }) {
    return api.post<WriteFileResponse>('/api/workspace/upload', opts)
  },

  search(opts: {
    root: string
    query: string
    limit?: number
    showHidden?: boolean
    kind?: 'name' | 'content'
    caseSensitive?: boolean
    wholeWord?: boolean
    regex?: boolean
    maxFileSizeBytes?: number
  }) {
    return api.get<FileSearchResponse>(
      `/api/workspace/search${qs({
        root: opts.root,
        query: opts.query,
        limit: opts.limit,
        showHidden: opts.showHidden,
        kind: opts.kind,
        caseSensitive: opts.caseSensitive,
        wholeWord: opts.wholeWord,
        regex: opts.regex,
        maxFileSizeBytes: opts.maxFileSizeBytes,
      })}`,
    )
  },

  watch(
    root: string,
    onEvent: (event: WorkspaceWatchEvent) => void,
    onError?: (err: Event) => void,
    onReconnect?: () => void,
  ): () => void {
    if (typeof window === 'undefined' || typeof window.EventSource !== 'function') {
      return () => {}
    }
    const url = withAuthToken(
      `${getBaseUrl()}/api/workspace/watch${qs({ root: normalizeWatchRoot(root) })}`,
    )
    const subscriber: WatchSubscriber = { onEvent, onError, onReconnect }
    const channel = acquireWatchChannel(url)
    channel.subscribers.add(subscriber)
    let released = false
    return () => {
      if (released) return
      released = true
      channel.subscribers.delete(subscriber)
      if (channel.subscribers.size === 0) {
        releaseWatchChannel(url, channel)
      }
    }
  },
}

type WatchSubscriber = {
  onEvent: (event: WorkspaceWatchEvent) => void
  onError?: (err: Event) => void
  onReconnect?: () => void
}

type WatchChannel = {
  subscribers: Set<WatchSubscriber>
  source: EventSource | null
  retryTimer: ReturnType<typeof setTimeout> | null
  retryMs: number
  hadConnection: boolean
  disposed: boolean
}

const watchChannels = new Map<string, WatchChannel>()

function normalizeWatchRoot(root: string): string {
  const stripped = root.replace(/[\\/]+$/, '')
  if (!stripped || /^[A-Za-z]:$/.test(stripped)) return root
  return stripped
}

function acquireWatchChannel(url: string): WatchChannel {
  const existing = watchChannels.get(url)
  if (existing) return existing
  const channel: WatchChannel = {
    subscribers: new Set(),
    source: null,
    retryTimer: null,
    retryMs: 1000,
    hadConnection: false,
    disposed: false,
  }
  watchChannels.set(url, channel)
  connectWatchChannel(url, channel)
  return channel
}

function releaseWatchChannel(url: string, channel: WatchChannel): void {
  channel.disposed = true
  if (watchChannels.get(url) === channel) {
    watchChannels.delete(url)
  }
  if (channel.retryTimer) {
    clearTimeout(channel.retryTimer)
    channel.retryTimer = null
  }
  closeWatchSource(channel)
}

function closeWatchSource(channel: WatchChannel): void {
  if (!channel.source) return
  try {
    channel.source.close()
  } catch {
  }
  channel.source = null
}

function connectWatchChannel(url: string, channel: WatchChannel): void {
  if (channel.disposed) return
  closeWatchSource(channel)
  const source = new window.EventSource(url, { withCredentials: false })
  channel.source = source
  source.onmessage = (msg: MessageEvent) => {
    let data: WorkspaceWatchEvent | null = null
    try {
      const parsed = JSON.parse(msg.data) as WorkspaceWatchEvent
      if (parsed && typeof parsed.relPath === 'string' && typeof parsed.kind === 'string') {
        if (typeof parsed.fromRelPath !== 'string') {
          delete (parsed as { fromRelPath?: string }).fromRelPath
        }
        data = parsed
      }
    } catch {
    }
    if (!data) return
    for (const sub of Array.from(channel.subscribers)) {
      try {
        sub.onEvent(data)
      } catch {
      }
    }
  }
  source.onopen = () => {
    channel.retryMs = 1000
    if (channel.hadConnection) {
      for (const sub of Array.from(channel.subscribers)) {
        try {
          sub.onReconnect?.()
        } catch {
        }
      }
    }
    channel.hadConnection = true
  }
  source.onerror = (event: Event) => {
    if (channel.source !== source) return
    closeWatchSource(channel)
    for (const sub of Array.from(channel.subscribers)) {
      try {
        sub.onError?.(event)
      } catch {
      }
    }
    if (channel.disposed || channel.retryTimer) return
    channel.retryTimer = setTimeout(() => {
      channel.retryTimer = null
      channel.retryMs = Math.min(channel.retryMs * 2, 30_000)
      connectWatchChannel(url, channel)
    }, channel.retryMs)
  }
}
