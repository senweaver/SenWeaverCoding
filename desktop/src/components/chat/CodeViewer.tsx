// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useRef, useEffect, useMemo } from 'react'
import { ShikiHighlighter, createJavaScriptRegexEngine } from 'react-shiki'
import 'react-shiki/css'
import { CopyButton } from '../shared/CopyButton'

function countLinesFast(input: string): number {
  let n = 1
  const len = input.length
  for (let i = 0; i < len; i++) {
    if (input.charCodeAt(i) === 10) n++
  }
  return n
}

function sliceFirstNLines(input: string, n: number): string {
  if (n <= 0) return ''
  const len = input.length
  let count = 0
  for (let i = 0; i < len; i++) {
    if (input.charCodeAt(i) === 10) {
      count++
      if (count >= n) return input.slice(0, i)
    }
  }
  return input
}

function scheduleIdleCallback(cb: () => void): () => void {
  if (typeof window === 'undefined') {
    cb()
    return () => {}
  }
  type IdleHandle = number
  type IdleFn = (cb: () => void, options?: { timeout?: number }) => IdleHandle
  const ric = (window as unknown as { requestIdleCallback?: IdleFn }).requestIdleCallback
  if (typeof ric === 'function') {
    const handle = ric(cb, { timeout: 250 })
    const cancel = (window as unknown as { cancelIdleCallback?: (h: IdleHandle) => void })
      .cancelIdleCallback
    return () => {
      if (typeof cancel === 'function') cancel(handle)
    }
  }
  const t = setTimeout(cb, 80)
  return () => clearTimeout(t)
}

type Props = {
  code: string
  language?: string
  maxLines?: number
  showLineNumbers?: boolean
}

const warmCodeTheme = {
  name: 'warm-code',
  type: 'dark' as const,
  fg: 'var(--color-code-fg)',
  bg: 'transparent',
  tokenColors: [
    { scope: ['comment', 'punctuation.definition.comment'], settings: { foreground: 'var(--color-code-comment)', fontStyle: 'italic' } },
    { scope: ['string', 'string.quoted', 'string.template', 'string.other.link'], settings: { foreground: 'var(--color-code-string)' } },
    { scope: ['string.regexp'], settings: { foreground: 'var(--color-primary-container)' } },
    { scope: ['keyword', 'keyword.control', 'storage', 'storage.type', 'storage.modifier'], settings: { foreground: 'var(--color-code-keyword)' } },
    { scope: ['keyword.operator'], settings: { foreground: 'var(--color-code-keyword)' } },
    { scope: ['entity.name.function', 'support.function'], settings: { foreground: 'var(--color-code-function)' } },
    { scope: ['entity.name.type', 'support.type', 'support.class', 'entity.name.class', 'entity.other.inherited-class'], settings: { foreground: 'var(--color-code-type)' } },
    { scope: ['entity.name.type.parameter'], settings: { foreground: 'var(--color-code-number)' } },
    { scope: ['variable', 'variable.other', 'variable.other.readwrite'], settings: { foreground: 'var(--color-code-fg)' } },
    { scope: ['variable.parameter'], settings: { foreground: 'var(--color-code-parameter)' } },
    { scope: ['variable.other.property', 'support.type.property-name', 'meta.object-literal.key'], settings: { foreground: 'var(--color-code-property)' } },
    { scope: ['variable.other.constant', 'variable.other.enummember'], settings: { foreground: 'var(--color-code-type)' } },
    { scope: ['constant.numeric', 'constant.language'], settings: { foreground: 'var(--color-code-number)' } },
    { scope: ['punctuation', 'meta.brace', 'meta.bracket'], settings: { foreground: 'var(--color-code-punctuation)' } },
    { scope: ['entity.name.tag', 'punctuation.definition.tag'], settings: { foreground: 'var(--color-code-keyword)' } },
    { scope: ['entity.other.attribute-name'], settings: { foreground: 'var(--color-code-property)' } },
    { scope: ['meta.decorator', 'punctuation.decorator'], settings: { foreground: 'var(--color-code-type)' } },
    { scope: ['markup.inserted', 'punctuation.definition.inserted'], settings: { foreground: 'var(--color-code-inserted)' } },
    { scope: ['markup.deleted', 'punctuation.definition.deleted'], settings: { foreground: 'var(--color-code-deleted)' } },
    { scope: ['markup.heading', 'entity.name.section'], settings: { foreground: 'var(--color-code-function)', fontStyle: 'bold' } },
    { scope: ['markup.bold'], settings: { fontStyle: 'bold' } },
    { scope: ['markup.italic'], settings: { fontStyle: 'italic' } },
  ],
}

const CODE_AREA_PADDING = '0.5rem 12px'
const CODE_LINE_HEIGHT = 1.3
const shikiEngine = createJavaScriptRegexEngine({ forgiving: true })

function CodeArea({ code, language, showLineNumbers }: { code: string; language?: string; showLineNumbers: boolean }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [loaded, setLoaded] = useState(false)
  const [renderShiki, setRenderShiki] = useState(false)

  useEffect(() => {
    setRenderShiki(false)
    setLoaded(false)
    const cancel = scheduleIdleCallback(() => setRenderShiki(true))
    return cancel
  }, [code, language])

  useEffect(() => {
    if (!renderShiki) return

    const el = containerRef.current
    if (!el) return
    const check = () => {
      const shikiContainer = el.querySelector('[data-testid="shiki-container"]')

      if (shikiContainer?.querySelector('code')) {
        setLoaded(true)
      }
    }
    check()
    const observer = new MutationObserver(check)
    observer.observe(el, { childList: true, subtree: true })
    return () => observer.disconnect()
  }, [code, language, renderShiki])

  return (
    <div
      ref={containerRef}
      data-has-line-numbers={showLineNumbers ? 'true' : 'false'}
      className="code-viewer-area relative max-h-[420px] overflow-auto bg-[var(--color-code-bg)]"
    >
      {}
      {!loaded && (
        <pre
          style={{
            margin: 0,
            padding: CODE_AREA_PADDING,
            fontFamily: 'var(--font-mono)',
            fontSize: '12px',
            lineHeight: String(CODE_LINE_HEIGHT),
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            color: 'var(--color-code-fg)',
          }}
        >
          {code}
        </pre>
      )}
      {renderShiki && (
        <div
          data-code-viewer-content=""
          style={
            loaded
              ? { padding: CODE_AREA_PADDING }
              : {
                  position: 'absolute',
                  inset: 0,
                  opacity: 0,
                  pointerEvents: 'none',
                  padding: CODE_AREA_PADDING,
                }
          }
        >
          <ShikiHighlighter
            language={language || 'text'}
            theme={warmCodeTheme}
            engine={shikiEngine}
            showLineNumbers={showLineNumbers}
            showLanguage={false}
            addDefaultStyles={false}
            style={{
              margin: 0,
              fontFamily: 'var(--font-mono)',
              fontSize: '12px',
              lineHeight: String(CODE_LINE_HEIGHT),
            }}
          >
            {code}
          </ShikiHighlighter>
        </div>
      )}
    </div>
  )
}

export function CodeViewer({ code, language, maxLines = 20, showLineNumbers = false }: Props) {
  const [expanded, setExpanded] = useState(false)

  const totalLines = useMemo(() => countLinesFast(code), [code])
  const isTruncated = !expanded && totalLines > maxLines
  const visibleCode = useMemo(
    () => (isTruncated ? sliceFirstNLines(code, maxLines) : code),
    [code, isTruncated, maxLines],
  )

  const effectiveShowLineNumbers = showLineNumbers && !!language && language !== 'text'
  const languageLabel = language || 'code'
  const lineCountLabel = `${totalLines} ${totalLines === 1 ? 'line' : 'lines'}`
  const showExpandToggle = totalLines > maxLines

  return (
    <div className="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-outline-variant)]/50 bg-[var(--color-surface-container-low)]">
      {}
      <div className="flex items-center justify-between border-b border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] px-3 py-1.5 text-[11px] text-[var(--color-text-tertiary)]">
        <div className="flex items-center gap-3">
          <span className="font-semibold uppercase tracking-[0.14em]">{languageLabel}</span>
          <span>{lineCountLabel}</span>
        </div>
        <CopyButton
          text={code}
          className="rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] px-2 py-1 text-[11px] text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
        />
      </div>

      {}
      <CodeArea
        code={visibleCode}
        language={language}
        showLineNumbers={effectiveShowLineNumbers}
      />

      {}
      {showExpandToggle && (
        <button
          onClick={() => setExpanded((value) => !value)}
          className="w-full border-t border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] py-1.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
        >
          {expanded ? 'Collapse' : `Show ${totalLines - maxLines} more lines`}
        </button>
      )}
    </div>
  )
}
