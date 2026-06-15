// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { parseMarkdown, type ParsedMarkdown } from './markdownParse'

const CACHE_CAP = 300

const cache = new Map<string, ParsedMarkdown>()

let worker: Worker | null = null
let workerFailed = false
let nextRequestId = 1
const pending = new Map<
  number,
  { resolve: (result: ParsedMarkdown) => void; content: string }
>()

function cacheGet(content: string): ParsedMarkdown | undefined {
  const value = cache.get(content)
  if (value !== undefined) {
    cache.delete(content)
    cache.set(content, value)
  }
  return value
}

function cachePut(content: string, value: ParsedMarkdown): void {
  if (cache.has(content)) cache.delete(content)
  cache.set(content, value)
  if (cache.size > CACHE_CAP) {
    const oldest = cache.keys().next().value
    if (oldest !== undefined) cache.delete(oldest)
  }
}

function parseFallback(content: string): ParsedMarkdown {
  const result = parseMarkdown(content)
  cachePut(content, result)
  return result
}

function drainPendingViaFallback(): void {
  const entries = Array.from(pending.values())
  pending.clear()
  for (const entry of entries) {
    entry.resolve(parseFallback(entry.content))
  }
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
      if (ok && result) {
        cachePut(entry.content, result)
        entry.resolve(result)
      } else {
        entry.resolve(parseFallback(entry.content))
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
  return cacheGet(content)
}

export function parseMarkdownAsync(content: string): Promise<ParsedMarkdown> {
  const cached = cacheGet(content)
  if (cached) return Promise.resolve(cached)
  const w = ensureWorker()
  if (!w) return Promise.resolve(parseFallback(content))
  return new Promise((resolve) => {
    const id = nextRequestId++
    pending.set(id, { resolve, content })
    w.postMessage({ id, content })
  })
}
