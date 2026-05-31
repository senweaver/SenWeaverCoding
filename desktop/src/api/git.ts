// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { ApiError, getBaseUrl } from './client'

export type GitStatusEntry = {
  relPath: string
  index: string
  worktree: string
  origRelPath?: string
}

export type GitStatusResponse = {
  isRepo: boolean
  entries: GitStatusEntry[]
  computedAt: number
  etag?: string
}

export type GitStatusFetchResult =
  | {
      notModified: true
      etag: string
    }
  | {
      notModified: false
      data: GitStatusResponse
      etag: string
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

function stripQuotes(value: string): string {
  return value.trim().replace(/^"|"$/g, '')
}

export const gitApi = {
  async fetchStatus(opts: {
    root: string
    forceRefresh?: boolean
    etag?: string
  }): Promise<GitStatusFetchResult> {
    const url = `${getBaseUrl()}/api/git/status${qs({
      root: opts.root,
      forceRefresh: opts.forceRefresh,
    })}`
    const headers: Record<string, string> = {}
    if (opts.etag && !opts.forceRefresh) {
      headers['If-None-Match'] = `"${stripQuotes(opts.etag)}"`
    }
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort(), 30_000)
    try {
      const res = await fetch(url, {
        method: 'GET',
        headers,
        signal: controller.signal,
      })
      const rawEtag = res.headers.get('ETag') ?? ''
      const etag = stripQuotes(rawEtag)
      if (res.status === 304) {
        return { notModified: true, etag: etag || stripQuotes(opts.etag ?? '') }
      }
      if (!res.ok) {
        const errorBody = await res.json().catch(() => res.text())
        throw new ApiError(res.status, errorBody)
      }
      const data = (await res.json()) as GitStatusResponse
      const finalEtag = etag || stripQuotes(data.etag ?? '')
      return {
        notModified: false,
        data,
        etag: finalEtag,
      }
    } catch (err) {
      if (controller.signal.aborted) {
        throw new Error('Request timed out after 30s')
      }
      throw err
    } finally {
      clearTimeout(timeout)
    }
  },
}
