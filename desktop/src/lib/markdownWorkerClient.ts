// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { parseMarkdown, type ParsedMarkdown } from './markdownParse'

const CACHE_MAX_ENTRIES = 300
const CACHE_MAX_BYTES = 16 * 1024 * 1024
const PENDING_MAX = 64
const PENDING_TIMEOUT_MS = 10_000

type CacheEntry = { value: ParsedMarkdown; bytes: number }

const cache = new Map<string, CacheEntry>()
let cacheTotalBytes = 0

let worker: Worker | null = null
let workerFailed = false
let nextRequestId = 1

type PendingEntry = {
  resolve: (result: ParsedMarkdown) => void
  reject: (error: Error) => void
  content: string
  key: string
  timer: ReturnType<typeof setTimeout>
  cacheWrite: boolean
}

const pending = new Map<number, PendingEntry>()

function contentKey(content: string): string {
  let h1 = 5381
  let h2 = 52711
  for (let i = 0; i < content.length; i++) {
    const code = content.charCodeAt(i)
    h1 = Math.imul(h1, 33) ^ code
    h2 = Math.imul(h2, 31) ^ code
  }
  return `${(h1 >>> 0).toString(36)}-${(h2 >>> 0).toString(36)}-${content.length.toString(36)}`
}

function cacheGet(key: string): ParsedMarkdown | undefined {
  const entry = cache.get(key)
  if (entry === undefined) return undefined
  cache.delete(key)
  cache.set(key, entry)
  return entry.value
}

function cachePut(key: string, value: ParsedMarkdown, bytes: number): void {
  const existing = cache.get(key)
  if (existing !== undefined) {
    cacheTotalBytes -= existing.bytes
    cache.delete(key)
  }
  cache.set(key, { value, bytes })
  cacheTotalBytes += bytes
  while (
    (cache.size > CACHE_MAX_ENTRIES || cacheTotalBytes > CACHE_MAX_BYTES) &&
    cache.size > 1
  ) {
    const oldestKey = cache.keys().next().value
    if (oldestKey === undefined) break
    const oldest = cache.get(oldestKey)
    if (oldest !== undefined) cacheTotalBytes -= oldest.bytes
    cache.delete(oldestKey)
  }
}

function parseFallback(content: string, key: string, cacheWrite = true): ParsedMarkdown {
  const result = parseMarkdown(content)
  if (cacheWrite) cachePut(key, result, content.length * 2)
  return result
}

function drainPendingViaFallback(): void {
  const entries = Array.from(pending.values())
  pending.clear()
  for (const entry of entries) {
    clearTimeout(entry.timer)
    entry.resolve(parseFallback(entry.content, entry.key, entry.cacheWrite))
  }
}

function restartWorkerAfterTimeout(): void {
  const stalled = worker
  worker = null
  try {
    stalled?.terminate()
  } catch {
  }
  drainPendingViaFallback()
}

function ensureWorker(): Worker | null {
  if (workerFailed) return null
  if (worker) return worker
  try {
    worker = new Worker(new URL('../workers/markdown.worker.ts', import.meta.url), {
      type: 'module',
    })
    worker.onmessage = (
      event: MessageEvent<{ id: number; ok: boolean; result?: ParsedMarkdown }>,
    ) => {
      const { id, ok, result } = event.data
      const entry = pending.get(id)
      if (!entry) return
      pending.delete(id)
      clearTimeout(entry.timer)
      if (ok && result) {
        if (entry.cacheWrite) cachePut(entry.key, result, entry.content.length * 2)
        entry.resolve(result)
      } else {
        entry.resolve(parseFallback(entry.content, entry.key, entry.cacheWrite))
      }
    }
    worker.onerror = () => {
      workerFailed = true
      const failed = worker
      worker = null
      failed?.terminate()
      drainPendingViaFallback()
    }
  } catch {
    workerFailed = true
    worker = null
  }
  return worker
}

export function getCachedMarkdown(content: string): ParsedMarkdown | undefined {
  return cacheGet(contentKey(content))
}

const SYNC_PARSE_MAX_CHARS = 64 * 1024

export function getMarkdownForImmediateRender(
  content: string,
): ParsedMarkdown | undefined {
  const key = contentKey(content)
  const cached = cacheGet(key)
  if (cached) return cached
  if (content.length > SYNC_PARSE_MAX_CHARS) return undefined
  return parseFallback(content, key, true)
}

export function parseMarkdownAsync(
  content: string,
  options?: { cacheWrite?: boolean },
): Promise<ParsedMarkdown> {
  const cacheWrite = options?.cacheWrite !== false
  const key = contentKey(content)
  const cached = cacheGet(key)
  if (cached) return Promise.resolve(cached)
  const w = ensureWorker()
  if (!w) return Promise.resolve(parseFallback(content, key, cacheWrite))
  if (pending.size >= PENDING_MAX) return Promise.resolve(parseFallback(content, key, cacheWrite))
  const request = new Promise<ParsedMarkdown>((resolve, reject) => {
    const id = nextRequestId++
    const timer = setTimeout(() => {
      const entry = pending.get(id)
      if (!entry) return
      pending.delete(id)
      entry.reject(new Error('markdown worker request timed out'))
      restartWorkerAfterTimeout()
    }, PENDING_TIMEOUT_MS)
    pending.set(id, { resolve, reject, content, key, timer, cacheWrite })
    w.postMessage({ id, content })
  })
  return request.catch(() => parseFallback(content, key, cacheWrite))
}
