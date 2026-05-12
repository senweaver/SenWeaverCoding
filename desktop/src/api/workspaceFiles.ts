import { api } from './client'
import { getBaseUrl } from './client'
import type {
  FileContent,
  FileSearchResponse,
  FileTreeResponse,
  WriteFileResponse,
} from '../types/workspaceFile'

export type WorkspaceWatchEvent = {
  kind: 'created' | 'modified' | 'removed' | 'renamed'
  relPath: string
  fromRelPath?: string
}

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
  }) {
    return api.get<FileSearchResponse>(
      `/api/workspace/search${qs({
        root: opts.root,
        query: opts.query,
        limit: opts.limit,
        showHidden: opts.showHidden,
      })}`,
    )
  },

  watch(
    root: string,
    onEvent: (event: WorkspaceWatchEvent) => void,
    onError?: (err: Event) => void,
  ): () => void {
    if (typeof window === 'undefined' || typeof window.EventSource !== 'function') {
      return () => {}
    }
    const url = `${getBaseUrl()}/api/workspace/watch${qs({ root })}`
    const source = new window.EventSource(url, { withCredentials: false })
    source.onmessage = (msg: MessageEvent) => {
      try {
        const data = JSON.parse(msg.data) as WorkspaceWatchEvent
        if (data && typeof data.relPath === 'string' && typeof data.kind === 'string') {
          if (typeof data.fromRelPath !== 'string') {
            delete (data as { fromRelPath?: string }).fromRelPath
          }
          onEvent(data)
        }
      } catch {

      }
    }
    if (onError) {
      source.onerror = onError
    }
    return () => {
      try {
        source.close()
      } catch {

      }
    }
  },
}
