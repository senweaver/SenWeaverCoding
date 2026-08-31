// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useRef } from 'react'

type Props = {
  content: string
  className?: string
}

const URL_RE = /(https?:\/\/[^\s<>]+)/g
const INLINE_CODE_RE = /`([^`\n]+)`/g
const BOLD_RE = /\*\*([^*\n]+)\*\*/g
const ITALIC_RE = /(^|[^*])\*([^*\n]+)\*/g

function escapeHtml(input: string): string {
  return input
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function applyInlineFormatting(escaped: string): string {
  let html = escaped.replace(INLINE_CODE_RE, (_m, code: string) => {
    return `<code class="md-stream-code">${code}</code>`
  })
  html = html.replace(BOLD_RE, '<strong>$1</strong>')
  html = html.replace(ITALIC_RE, '$1<em>$2</em>')
  html = html.replace(URL_RE, (url) => {
    return `<a href="${url}" target="_blank" rel="noreferrer noopener">${url}</a>`
  })
  return html
}

function renderSegment(seg: string): string {
  if (!seg) return ''
  if (seg.startsWith('```')) {
    const closed = seg.endsWith('```') && seg.length > 3
    const inner = closed ? seg.slice(3, -3) : seg.slice(3)
    const firstNl = inner.indexOf('\n')
    const body = firstNl >= 0 ? inner.slice(firstNl + 1) : inner
    const lang = firstNl >= 0 ? inner.slice(0, firstNl).trim() : ''
    const langAttr = lang ? ` data-lang="${escapeHtml(lang)}"` : ''
    return `<pre class="md-stream-pre"${langAttr}><code>${escapeHtml(body)}</code></pre>`
  }
  const escaped = escapeHtml(seg)
  return applyInlineFormatting(escaped).replace(/\n/g, '<br />')
}

type Segment = { end: number; closed: boolean }

function nextSegment(content: string, start: number): Segment | null {
  if (start >= content.length) return null
  if (content.startsWith('```', start)) {
    const close = content.indexOf('```', start + 3)
    if (close === -1) return { end: content.length, closed: false }
    return { end: close + 3, closed: true }
  }
  const next = content.indexOf('```', start)
  if (next === -1) return { end: content.length, closed: false }
  return { end: next, closed: true }
}

type StreamCache = {
  content: string
  committedHtml: string
  committedLen: number
  pendingHtml: string
}

function emptyCache(): StreamCache {
  return { content: '', committedHtml: '', committedLen: 0, pendingHtml: '' }
}

function buildIncremental(cache: StreamCache, content: string): StreamCache {
  if (content === cache.content) return cache
  if (!content.startsWith(cache.content)) {
    cache = emptyCache()
  }
  let cursor = cache.committedLen
  let committedHtml = cache.committedHtml
  while (true) {
    const seg = nextSegment(content, cursor)
    if (!seg || !seg.closed) break
    committedHtml += renderSegment(content.slice(cursor, seg.end))
    cursor = seg.end
  }
  if (!content.startsWith('```', cursor)) {
    const pendingSlice = content.slice(cursor)
    const lastBreak = pendingSlice.lastIndexOf('\n\n')
    if (lastBreak > 0) {
      committedHtml += renderSegment(pendingSlice.slice(0, lastBreak + 2))
      cursor += lastBreak + 2
    }
  }
  return {
    content,
    committedHtml,
    committedLen: cursor,
    pendingHtml: renderSegment(content.slice(cursor)),
  }
}

export function StreamingMarkdownRenderer({ content, className }: Props) {
  const cacheRef = useRef<StreamCache>(emptyCache())
  cacheRef.current = buildIncremental(cacheRef.current, content)
  const { committedHtml, pendingHtml } = cacheRef.current
  return (
    <div className={`streaming-markdown ${className ?? ''}`.trim()}>
      {committedHtml ? (
        <div dangerouslySetInnerHTML={{ __html: committedHtml }} />
      ) : null}
      {pendingHtml ? (
        <div dangerouslySetInnerHTML={{ __html: pendingHtml }} />
      ) : null}
    </div>
  )
}
