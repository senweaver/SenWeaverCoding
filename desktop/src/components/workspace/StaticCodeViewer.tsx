// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { ShikiHighlighter, createJavaScriptRegexEngine } from 'react-shiki'
import 'react-shiki/css'
import { useTranslation } from '../../i18n'
import { ConfirmDialog } from '../shared/ConfirmDialog'

type Props = {
  code: string
  language?: string
  maxLines?: number
  initialLines?: number
  showLineNumbers?: boolean
  className?: string
}

const DEFAULT_CHUNK = 5000

const SHOW_ALL_CONFIRM_THRESHOLD = 200_000

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

const shikiEngine = createJavaScriptRegexEngine({ forgiving: true })

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

export function StaticCodeViewer({
  code,
  language,
  maxLines = DEFAULT_CHUNK,
  initialLines,
  showLineNumbers = true,
  className,
}: Props) {
  const t = useTranslation()
  const totalLines = useMemo(() => countLinesFast(code), [code])
  const startLimit = Math.max(1, initialLines ?? maxLines)
  const [visibleLimit, setVisibleLimit] = useState<number>(startLimit)
  const limitRef = useRef(visibleLimit)
  limitRef.current = visibleLimit

  useEffect(() => {
    setVisibleLimit(startLimit)
  }, [code, startLimit])

  const isTruncated = totalLines > visibleLimit
  const visibleCode = useMemo(
    () => (isTruncated ? sliceFirstNLines(code, visibleLimit) : code),
    [code, isTruncated, visibleLimit],
  )

  const lang = language && language !== 'plaintext' ? language : 'text'
  const remaining = Math.max(0, totalLines - visibleLimit)
  const nextChunk = Math.min(maxLines, remaining)

  const [showAllConfirmOpen, setShowAllConfirmOpen] = useState(false)

  const onShowMore = () => {
    setVisibleLimit((v) => Math.min(totalLines, v + maxLines))
  }
  const onShowAll = () => {
    if (totalLines > SHOW_ALL_CONFIRM_THRESHOLD) {
      setShowAllConfirmOpen(true)
      return
    }
    setVisibleLimit(totalLines)
  }
  const onConfirmShowAll = () => {
    setShowAllConfirmOpen(false)
    setVisibleLimit(totalLines)
  }
  const onCancelShowAll = () => {
    setShowAllConfirmOpen(false)
  }

  return (
    <div
      className={
        className ??
        'flex h-full min-h-0 flex-col overflow-hidden bg-[var(--color-code-bg)]'
      }
    >
      <div className="flex flex-shrink-0 items-center gap-3 border-b border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] px-3 py-1.5 text-[11px] text-[var(--color-text-tertiary)]">
        <span className="font-semibold uppercase tracking-[0.14em]">
          {lang}
        </span>
        <span>
          {t('codeViewer.truncated', {
            shown: visibleLimit.toLocaleString(),
            total: totalLines.toLocaleString(),
          })}
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <div
          data-static-code-viewer-content=""
          style={{ padding: '0.5rem 12px' }}
        >
          <ShikiHighlighter
            language={lang}
            theme={warmCodeTheme}
            engine={shikiEngine}
            showLineNumbers={showLineNumbers}
            showLanguage={false}
            addDefaultStyles={false}
            style={{
              margin: 0,
              fontFamily: 'var(--font-mono)',
              fontSize: '12px',
              lineHeight: '1.4',
            }}
          >
            {visibleCode}
          </ShikiHighlighter>
        </div>
      </div>
      {isTruncated && (
        <div className="flex flex-shrink-0 items-center justify-center gap-2 border-t border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] px-3 py-2 text-[11px]">
          <button
            type="button"
            onClick={onShowMore}
            className="rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] px-3 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
          >
            {t('codeViewer.showNext', {
              count: nextChunk.toLocaleString(),
            })}
          </button>
          <button
            type="button"
            onClick={onShowAll}
            className="rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] px-3 py-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
          >
            {t('codeViewer.showAll', {
              count: remaining.toLocaleString(),
            })}
          </button>
        </div>
      )}
      <ConfirmDialog
        open={showAllConfirmOpen}
        onClose={onCancelShowAll}
        onConfirm={onConfirmShowAll}
        title={t('codeViewer.showAllConfirm.title')}
        body={t('codeViewer.showAllConfirm.body', {
          total: totalLines.toLocaleString(),
        })}
        confirmLabel={t('codeViewer.showAllConfirm.confirm')}
        cancelLabel={t('codeViewer.showAllConfirm.cancel')}
        confirmVariant="primary"
      />
    </div>
  )
}
