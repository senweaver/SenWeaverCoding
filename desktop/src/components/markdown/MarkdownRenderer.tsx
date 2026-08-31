// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { memo, useMemo, useCallback, useState, useEffect, useRef, lazy, Suspense } from 'react'
import DOMPurify from 'dompurify'
import { CodeViewer } from '../chat/CodeViewer'
import { isMermaidBlock } from '../../lib/mermaidDetect'
import type { CodeBlock, ParsedMarkdown } from '../../lib/markdownParse'
import {
  getCachedMarkdown,
  getMarkdownForImmediateRender,
  parseMarkdownAsync,
} from '../../lib/markdownWorkerClient'
import { useTranslation } from '../../i18n'
import {
  isScrollActive,
  runWhenScrollQuiet,
  getChatScrollerWidth,
  onChatScrollerWidthChange,
} from '../../lib/scrollActivity'
import { StreamingMarkdownRenderer } from './StreamingMarkdownRenderer'

const MermaidRenderer = lazy(() =>
  import('../chat/MermaidRenderer').then((m) => ({ default: m.MermaidRenderer })),
)

type Props = {
  content: string
  variant?: 'default' | 'document'
  className?: string

  scale?: 'default' | 'chat'

  streaming?: boolean
}

function shouldRenderAsMermaid(block: CodeBlock): boolean {
  return isMermaidBlock(block.language, block.code)
}

const ENHANCE_CACHE = new Map<string, string>()
const ENHANCE_CACHE_MAX = 400

const MEASURED_HEIGHT_CACHE = new Map<string, number>()
const MEASURED_HEIGHT_CACHE_MAX = 600

function contentHeightKey(content: string, variant: string, scale: string): string {
  let h1 = 5381
  let h2 = 52711
  for (let i = 0; i < content.length; i++) {
    const code = content.charCodeAt(i)
    h1 = Math.imul(h1, 33) ^ code
    h2 = Math.imul(h2, 31) ^ code
  }
  return `${variant}:${scale}:${(h1 >>> 0).toString(36)}-${(h2 >>> 0).toString(36)}-${content.length.toString(36)}`
}

function getMeasuredHeight(key: string): number | undefined {
  const value = MEASURED_HEIGHT_CACHE.get(key)
  if (value !== undefined) {
    MEASURED_HEIGHT_CACHE.delete(key)
    MEASURED_HEIGHT_CACHE.set(key, value)
  }
  return value
}

function setMeasuredHeight(key: string, height: number): void {
  const existing = MEASURED_HEIGHT_CACHE.get(key)
  if (existing === height) return
  if (existing === undefined && MEASURED_HEIGHT_CACHE.size >= MEASURED_HEIGHT_CACHE_MAX) {
    const oldest = MEASURED_HEIGHT_CACHE.keys().next().value
    if (oldest !== undefined) MEASURED_HEIGHT_CACHE.delete(oldest)
  }
  MEASURED_HEIGHT_CACHE.set(key, height)
  schedulePersistHeights()
}

const HEIGHT_STORE_KEY = 'sen.mdHeights.v2'
let persistHeightsTimer: number | null = null
let measureWidth = 0
let pendingPersisted: { w: number; e: Array<[string, number]> } | null = null

function persistHeightsNow(): void {
  if (measureWidth <= 0) return
  try {
    window.localStorage.setItem(
      HEIGHT_STORE_KEY,
      JSON.stringify({
        w: measureWidth,
        e: Array.from(MEASURED_HEIGHT_CACHE.entries()),
      }),
    )
  } catch {
    return
  }
}

function schedulePersistHeights(): void {
  if (typeof window === 'undefined') return
  if (persistHeightsTimer !== null) return
  persistHeightsTimer = window.setTimeout(() => {
    persistHeightsTimer = null
    persistHeightsNow()
  }, 1500)
}

if (typeof window !== 'undefined') {
  try {
    const raw = window.localStorage.getItem(HEIGHT_STORE_KEY)
    if (raw) {
      const data = JSON.parse(raw) as { w?: number; e?: Array<[string, number]> }
      if (typeof data.w === 'number' && data.w > 0 && Array.isArray(data.e)) {
        pendingPersisted = { w: data.w, e: data.e }
      }
    }
  } catch {
    pendingPersisted = null
  }
  onChatScrollerWidthChange((width) => {
    if (measureWidth > 0 && width !== measureWidth) {
      MEASURED_HEIGHT_CACHE.clear()
      if (persistHeightsTimer !== null) {
        window.clearTimeout(persistHeightsTimer)
        persistHeightsTimer = null
      }
      try {
        window.localStorage.removeItem(HEIGHT_STORE_KEY)
      } catch {
        return
      }
    }
    measureWidth = width
    if (pendingPersisted) {
      if (pendingPersisted.w === width) {
        for (const entry of pendingPersisted.e) {
          if (
            Array.isArray(entry) &&
            typeof entry[0] === 'string' &&
            typeof entry[1] === 'number'
          ) {
            MEASURED_HEIGHT_CACHE.set(entry[0], entry[1])
          }
        }
      }
      pendingPersisted = null
    }
  })
  window.addEventListener('pagehide', () => {
    if (persistHeightsTimer === null) return
    window.clearTimeout(persistHeightsTimer)
    persistHeightsTimer = null
    persistHeightsNow()
  })
}

function estimateMarkdownHeight(text: string): number {
  let lines = 1
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) lines++
  }
  const width = getChatScrollerWidth()
  const charsPerLine =
    width > 0 ? Math.min(140, Math.max(48, Math.floor(width / 8))) : 90
  const wrapped = Math.max(lines, Math.ceil(text.length / charsPerLine))
  return Math.min(1600, Math.max(40, wrapped * 24 + 12))
}

function enhanceMarkdownHtml(html: string, cacheWrite = true): string {
  const cached = ENHANCE_CACHE.get(html)
  if (cached !== undefined) {
    ENHANCE_CACHE.delete(html)
    ENHANCE_CACHE.set(html, cached)
    return cached
  }

  const cleanHtml = DOMPurify.sanitize(html, {
    ADD_TAGS: ['use'],
    ADD_ATTR: ['xlink:href'],
  })

  if (typeof document === 'undefined') {
    return cleanHtml
  }

  const container = document.createElement('div')
  container.innerHTML = cleanHtml

  container.querySelectorAll('table').forEach((table) => {
    if (table.parentElement?.classList.contains('md-table-wrap')) return
    const wrapper = document.createElement('div')
    wrapper.className = 'md-table-wrap'
    table.parentNode?.insertBefore(wrapper, table)
    wrapper.appendChild(table)
  })

  container.querySelectorAll('a[href]').forEach((link) => {
    link.setAttribute('target', '_blank')
    link.setAttribute('rel', 'noreferrer noopener')
  })

  const result = container.innerHTML
  if (cacheWrite) {
    if (ENHANCE_CACHE.size >= ENHANCE_CACHE_MAX) {
      const oldest = ENHANCE_CACHE.keys().next().value
      if (oldest !== undefined) ENHANCE_CACHE.delete(oldest)
    }
    ENHANCE_CACHE.set(html, result)
  }
  return result
}

const BASE_PROSE_CLASSES = `markdown-prose prose prose-sm max-w-none text-[var(--color-text-primary)]
  prose-headings:text-[var(--color-text-primary)] prose-headings:font-semibold
  prose-p:my-2 prose-p:leading-relaxed
  prose-p:break-words
  prose-code:text-[13px] prose-code:text-[var(--color-code-fg)] prose-code:font-[var(--font-mono)] prose-code:bg-[var(--color-code-bg)] prose-code:border prose-code:border-[var(--color-border)] prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded-md prose-code:before:hidden prose-code:after:hidden
  prose-pre:!bg-transparent prose-pre:!p-0 prose-pre:!shadow-none
  prose-a:text-[var(--color-text-accent)] prose-a:no-underline hover:prose-a:underline
  prose-strong:text-[var(--color-text-primary)]
  prose-ul:my-2 prose-ol:my-2
  prose-li:my-0.5
  prose-table:my-0 prose-table:w-full prose-table:table-auto prose-table:text-sm
  prose-th:bg-[var(--color-surface-info)] prose-th:px-3 prose-th:py-2 prose-th:text-left prose-th:whitespace-normal prose-th:break-words prose-th:align-top prose-th:border-b prose-th:border-[var(--color-border)]
  prose-td:px-3 prose-td:py-2 prose-td:border-b prose-td:border-[var(--color-border)] prose-td:whitespace-normal prose-td:break-words prose-td:align-top prose-td:bg-[var(--color-surface)]
  [&_.md-table-wrap]:my-5 [&_.md-table-wrap]:overflow-x-auto [&_.md-table-wrap]:rounded-xl [&_.md-table-wrap]:border [&_.md-table-wrap]:border-[var(--color-border)] [&_.md-table-wrap]:bg-[var(--color-surface-container-lowest)]`

const DOCUMENT_PROSE_CLASSES = `
  prose-p:text-[15px] prose-p:leading-7
  prose-headings:scroll-mt-6 prose-headings:tracking-[-0.01em]
  prose-h1:mb-4 prose-h1:text-2xl prose-h1:font-semibold prose-h1:leading-tight
  prose-h2:mt-8 prose-h2:mb-3 prose-h2:border-b prose-h2:border-[var(--color-border)] prose-h2:pb-2 prose-h2:text-xl prose-h2:font-semibold
  prose-h3:mt-6 prose-h3:mb-2 prose-h3:text-base prose-h3:font-semibold
  prose-h4:mt-5 prose-h4:mb-2 prose-h4:text-sm prose-h4:font-semibold
  prose-blockquote:my-4 prose-blockquote:rounded-r-lg prose-blockquote:border-l-4 prose-blockquote:border-[var(--color-outline-variant)] prose-blockquote:bg-[var(--color-surface-container-low)] prose-blockquote:px-4 prose-blockquote:py-2 prose-blockquote:italic
  prose-hr:my-6 prose-hr:border-[var(--color-border)]
  prose-img:rounded-lg prose-img:border prose-img:border-[var(--color-border)]
  prose-kbd:rounded prose-kbd:border prose-kbd:border-[var(--color-border)] prose-kbd:bg-[var(--color-surface-container-lowest)] prose-kbd:px-1.5 prose-kbd:py-0.5 prose-kbd:font-[var(--font-mono)] prose-kbd:text-[12px] prose-kbd:font-normal prose-kbd:text-[var(--color-text-secondary)] prose-kbd:shadow-none
  prose-ul:pl-5 prose-ul:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-ol:pl-5 prose-ol:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-li:my-1.5
  prose-table:my-0`

const CHAT_BODY_SCALE_CLASSES = `
  !text-[13px]
  prose-p:!text-[13px] prose-p:!leading-relaxed
  prose-li:!text-[13px]
  prose-code:!text-[12px]
  prose-table:!text-[12px]
  prose-th:!text-[12px] prose-td:!text-[12px]
`

const CHAT_HEADING_DEFAULT_CLASSES = `
  prose-h1:!text-base prose-h2:!text-[15px] prose-h3:!text-[14px] prose-h4:!text-[13px]
`

const CHAT_DOCUMENT_PROSE_CLASSES = `
  prose-p:!text-[13px] prose-p:!leading-[1.62]
  prose-headings:scroll-mt-6 prose-headings:tracking-[-0.01em]
  prose-h1:!mb-3 prose-h1:!text-xl prose-h1:!font-semibold prose-h1:!leading-tight
  prose-h2:!mt-6 prose-h2:!mb-2 prose-h2:border-b prose-h2:border-[var(--color-border)] prose-h2:!pb-2 prose-h2:!text-lg prose-h2:!font-semibold
  prose-h3:!mt-5 prose-h3:!mb-1.5 prose-h3:!text-[13px] prose-h3:!font-semibold
  prose-h4:!mt-4 prose-h4:!mb-1.5 prose-h4:!text-[13px] prose-h4:!font-semibold
  prose-blockquote:my-3 prose-blockquote:rounded-r-lg prose-blockquote:border-l-4 prose-blockquote:border-[var(--color-outline-variant)] prose-blockquote:bg-[var(--color-surface-container-low)] prose-blockquote:px-3 prose-blockquote:py-2 prose-blockquote:italic
  prose-hr:!my-5 prose-hr:border-[var(--color-border)]
  prose-img:rounded-lg prose-img:border prose-img:border-[var(--color-border)]
  prose-kbd:rounded prose-kbd:border prose-kbd:border-[var(--color-border)] prose-kbd:bg-[var(--color-surface-container-lowest)] prose-kbd:px-1.5 prose-kbd:py-0.5 prose-kbd:font-[var(--font-mono)] prose-kbd:!text-[11px] prose-kbd:font-normal prose-kbd:text-[var(--color-text-secondary)] prose-kbd:shadow-none
  prose-ul:pl-5 prose-ul:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-ol:pl-5 prose-ol:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-li:my-1
  prose-table:my-0`

function hashHtmlPartKey(content: string): string {
  let hash = 5381
  const max = Math.min(content.length, 128)
  for (let i = 0; i < max; i++) {
    hash = ((hash << 5) + hash + content.charCodeAt(i)) | 0
  }
  return (hash >>> 0).toString(36)
}

function getProseClasses(
  variant: 'default' | 'document',
  className?: string,
  scale: 'default' | 'chat' = 'default',
) {
  const chunks: string[] = [BASE_PROSE_CLASSES]

  if (scale === 'chat') {
    chunks.push(CHAT_BODY_SCALE_CLASSES)
  }

  if (variant === 'document') {
    chunks.push(scale === 'chat' ? CHAT_DOCUMENT_PROSE_CLASSES : DOCUMENT_PROSE_CLASSES)
  } else if (scale === 'chat') {
    chunks.push(CHAT_HEADING_DEFAULT_CLASSES)
  }

  if (className) chunks.push(className)

  return chunks.filter(Boolean).join(' ')
}

const MARKDOWN_RENDER_CHAR_CAP = 20_000

function MarkdownSkeleton({ height }: { height: number }) {
  const bars = Math.max(1, Math.min(28, Math.floor((height - 8) / 26)))
  return (
    <div className="flex flex-col gap-3 py-1" aria-hidden>
      {Array.from({ length: bars }, (_, i) => (
        <div
          key={i}
          className="h-[14px] animate-pulse rounded bg-[var(--color-surface-container-low)]"
          style={{ width: i === bars - 1 ? '62%' : '100%' }}
        />
      ))}
    </div>
  )
}

function MarkdownRendererInner({ content, variant = 'default', className, scale = 'default', streaming = false }: Props) {
  const t = useTranslation()
  const hostRef = useRef<HTMLDivElement>(null)
  const [inView, setInView] = useState(
    () =>
      streaming ||
      getCachedMarkdown(
        content.length > MARKDOWN_RENDER_CHAR_CAP
          ? content.slice(0, MARKDOWN_RENDER_CHAR_CAP)
          : content,
      ) !== undefined,
  )
  const [expanded, setExpanded] = useState(false)
  const overCap = content.length > MARKDOWN_RENDER_CHAR_CAP
  const visibleContent = !expanded && overCap ? content.slice(0, MARKDOWN_RENDER_CHAR_CAP) : content

  const heightKey = useMemo(
    () => contentHeightKey(visibleContent, variant, scale),
    [visibleContent, variant, scale],
  )

  useEffect(() => {
    if (streaming) {
      setInView(true)
      return
    }
    if (inView) return
    const el = hostRef.current
    if (!el) return
    let cancelQuiet: (() => void) | null = null
    const io = new IntersectionObserver(
      (entries) => {
        const hit = entries.find((entry) => entry.isIntersecting)
        if (!hit) return
        io.disconnect()
        const zeroShift =
          getCachedMarkdown(visibleContent) !== undefined &&
          MEASURED_HEIGHT_CACHE.has(heightKey)
        if (zeroShift || !isScrollActive()) {
          setInView(true)
          return
        }
        cancelQuiet = runWhenScrollQuiet(() => {
          cancelQuiet = null
          setInView(true)
        })
      },
      { rootMargin: '720px 0px' },
    )
    io.observe(el)
    return () => {
      io.disconnect()
      if (cancelQuiet) cancelQuiet()
    }
  }, [streaming, inView, visibleContent, heightKey])

  const [parsed, setParsed] = useState<{ source: string; result: ParsedMarkdown } | null>(null)

  useEffect(() => {
    if (!inView) return
    const cached = getCachedMarkdown(visibleContent)
    if (cached) {
      setParsed((prev) =>
        prev && prev.source === visibleContent ? prev : { source: visibleContent, result: cached },
      )
      return
    }
    const eager = getMarkdownForImmediateRender(visibleContent, {
      cacheWrite: !streaming,
    })
    if (eager) {
      setParsed({ source: visibleContent, result: eager })
    }
    let stale = false
    void parseMarkdownAsync(visibleContent, { cacheWrite: !streaming }).then((result) => {
      if (!stale) setParsed({ source: visibleContent, result })
    })
    return () => {
      stale = true
    }
  }, [inView, visibleContent, streaming])

  const active = parsed && parsed.source === visibleContent ? parsed.result : null
  const rendered = active ?? parsed?.result ?? null
  const pendingSuffix =
    !active && parsed && visibleContent.startsWith(parsed.source)
      ? visibleContent.slice(parsed.source.length)
      : ''
  const html = rendered?.html ?? ''
  const cachedHeight = streaming ? undefined : getMeasuredHeight(heightKey)
  const estimatedHeight = estimateMarkdownHeight(visibleContent)
  const stableBoxHeight = cachedHeight ?? estimatedHeight

  useEffect(() => {
    if (streaming || !inView) return
    if (!html || pendingSuffix) return
    if (!parsed || parsed.source !== visibleContent) return
    const el = hostRef.current
    if (!el) return
    if (parsed.result.codeBlocks.some(shouldRenderAsMermaid)) return
    const measured = el.offsetHeight
    if (measured > 0 && el.querySelector('img') === null) {
      setMeasuredHeight(heightKey, measured)
    }
  }, [streaming, inView, html, pendingSuffix, parsed, visibleContent, heightKey])
  const codeBlocks = useMemo(() => rendered?.codeBlocks ?? [], [rendered])
  const proseClasses = useMemo(
    () => getProseClasses(variant, className, scale),
    [variant, className, scale],
  )

  const parts = useMemo(() => {
    const raw: Array<{ type: 'html'; content: string } | { type: 'code'; block: CodeBlock }> = []
    if (codeBlocks.length === 0) {
      raw.push({ type: 'html' as const, content: html })
    } else {
      let remaining = html

      for (const block of codeBlocks) {
        const marker = `<div data-codeblock-id="${block.id}"></div>`
        const idx = remaining.indexOf(marker)
        if (idx === -1) continue

        const before = remaining.slice(0, idx)
        if (before) {
          raw.push({ type: 'html', content: before })
        }
        raw.push({ type: 'code', block })
        remaining = remaining.slice(idx + marker.length)
      }

      if (remaining) {
        raw.push({ type: 'html', content: remaining })
      }
    }

    const seen = new Map<string, number>()
    return raw.map((part) => {
      if (part.type === 'code') {
        return { part, key: `code:${part.block.id}` }
      }
      const h = hashHtmlPartKey(part.content)
      const occurrence = (seen.get(h) ?? 0) + 1
      seen.set(h, occurrence)
      return { part, key: `html:${h}:${occurrence}` }
    })
  }, [html, codeBlocks])

  const handleClick = useCallback(async (event: React.MouseEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement | null
    const button = target?.closest<HTMLButtonElement>('[data-copy-code]')
    if (!button) return

    const text = button.getAttribute('data-copy-code')
    if (!text) return

    try {
      await navigator.clipboard.writeText(text)
      const original = button.textContent
      button.textContent = 'Copied'
      window.setTimeout(() => {
        button.textContent = original
      }, 1500)
    } catch {

    }
  }, [])

  const expandControl = overCap ? (
    <button
      type="button"
      className="mt-2 text-[12px] text-[var(--color-text-secondary)]"
      onClick={() => setExpanded((prev) => !prev)}
    >
      {expanded ? t('common.collapse') : t('common.expand')}
    </button>
  ) : null

  if (!inView) {
    return (
      <div
        ref={hostRef}
        style={{ height: stableBoxHeight, overflow: 'hidden' }}
      >
        <MarkdownSkeleton height={stableBoxHeight} />
      </div>
    )
  }

  if (!rendered) {
    if (streaming) {
      return (
        <div ref={hostRef} className={proseClasses}>
          <StreamingMarkdownRenderer content={visibleContent} />
          {expandControl}
        </div>
      )
    }
    return (
      <div
        ref={hostRef}
        style={{ height: stableBoxHeight, overflow: 'hidden' }}
      >
        <MarkdownSkeleton height={stableBoxHeight} />
      </div>
    )
  }

  if (codeBlocks.length === 0) {
    const cleanHtml = enhanceMarkdownHtml(html, !streaming)
    return (
      <div ref={hostRef}>
        <div
          className={proseClasses}
          dangerouslySetInnerHTML={{ __html: cleanHtml }}
          onClick={handleClick}
        />
        {streaming && pendingSuffix && (
          <div className={proseClasses}>
            <StreamingMarkdownRenderer content={pendingSuffix} />
          </div>
        )}
        {expandControl}
      </div>
    )
  }

  return (
    <div ref={hostRef} className={proseClasses} onClick={handleClick}>
      {parts.map(({ part, key }) =>
        part.type === 'html' ? (
          <div key={key} dangerouslySetInnerHTML={{ __html: enhanceMarkdownHtml(part.content, !streaming) }} />
        ) : shouldRenderAsMermaid(part.block) ? (
          <Suspense
            key={key}
            fallback={
              <pre className="my-4 overflow-x-auto rounded bg-[var(--color-surface-container-low)] p-3 text-xs text-[var(--color-text-secondary)]">
                {part.block.code}
              </pre>
            }
          >
            <MermaidRenderer code={part.block.code} />
          </Suspense>
        ) : (
          <div key={key} className="my-4">
            <CodeViewer
              code={part.block.code}
              language={part.block.language}
            />
          </div>
        )
      )}
      {streaming && pendingSuffix && (
        <StreamingMarkdownRenderer content={pendingSuffix} />
      )}
      {expandControl}
    </div>
  )
}

export const MarkdownRenderer = memo(MarkdownRendererInner)
