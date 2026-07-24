// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { memo, useMemo, useState } from 'react'
import ReactDiffViewer, { DiffMethod } from 'react-diff-viewer-continued'
import { diffLines } from 'diff'
import { Highlight, type PrismTheme } from 'prism-react-renderer'
import { CopyButton } from '../shared/CopyButton'

type Props = {
  filePath: string
  oldString: string
  newString: string
}

function inferLanguage(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase()
  const langMap: Record<string, string> = {
    ts: 'typescript', tsx: 'tsx', js: 'javascript', jsx: 'jsx',
    py: 'python', rs: 'rust', go: 'go', rb: 'ruby',
    json: 'json', yaml: 'yaml', yml: 'yaml', toml: 'toml',
    md: 'markdown', css: 'css', html: 'markup', xml: 'markup',
    sql: 'sql', sh: 'bash', bash: 'bash', zsh: 'bash',
  }
  return langMap[ext ?? ''] || 'text'
}

const warmSyntaxTheme: PrismTheme = {
  plain: {
    color: 'var(--color-code-fg)',
    backgroundColor: 'transparent',
  },
  styles: [
    { types: ['comment', 'prolog', 'doctype', 'cdata'], style: { color: 'var(--color-code-comment)', fontStyle: 'italic' as const } },
    { types: ['string', 'attr-value', 'template-string'], style: { color: 'var(--color-code-string)' } },
    { types: ['keyword', 'selector', 'important', 'atrule'], style: { color: 'var(--color-code-keyword)' } },
    { types: ['function'], style: { color: 'var(--color-code-function)' } },
    { types: ['tag'], style: { color: 'var(--color-code-keyword)' } },
    { types: ['number', 'boolean'], style: { color: 'var(--color-code-number)' } },
    { types: ['operator'], style: { color: 'var(--color-code-fg)' } },
    { types: ['punctuation'], style: { color: 'var(--color-code-punctuation)' } },
    { types: ['variable', 'parameter'], style: { color: 'var(--color-code-fg)' } },
    { types: ['property', 'attr-name'], style: { color: 'var(--color-code-property)' } },
    { types: ['builtin', 'class-name', 'constant', 'symbol'], style: { color: 'var(--color-code-type)' } },
    { types: ['regex'], style: { color: 'var(--color-primary-container)' } },
    { types: ['inserted'], style: { color: 'var(--color-code-inserted)' } },
    { types: ['deleted'], style: { color: 'var(--color-code-deleted)' } },
  ],
}

function highlightSyntax(str: string, language: string) {
  return (
    <Highlight theme={warmSyntaxTheme} code={str} language={language}>
      {({ tokens, getTokenProps }) => (
        <>
          {tokens.map((line, i) => (
            <span key={i}>
              {line.map((token, key) => (
                <span key={key} {...getTokenProps({ token })} />
              ))}
            </span>
          ))}
        </>
      )}
    </Highlight>
  )
}

const diffStyles = {
  variables: {
    light: {
      diffViewerBackground: 'var(--color-code-bg)',
      diffViewerColor: 'var(--color-code-fg)',
      addedBackground: 'var(--color-diff-added-bg)',
      addedColor: 'var(--color-code-fg)',
      removedBackground: 'var(--color-diff-removed-bg)',
      removedColor: 'var(--color-code-fg)',
      wordAddedBackground: 'var(--color-diff-added-word)',
      wordRemovedBackground: 'var(--color-diff-removed-word)',
      addedGutterBackground: 'var(--color-diff-added-gutter)',
      removedGutterBackground: 'var(--color-diff-removed-gutter)',
      gutterBackground: 'var(--color-surface-container-low)',
      gutterBackgroundDark: 'var(--color-surface-container)',
      highlightBackground: 'var(--color-diff-highlight-bg)',
      highlightGutterBackground: 'var(--color-diff-highlight-gutter)',
      codeFoldGutterBackground: 'var(--color-surface-container-high)',
      codeFoldBackground: 'var(--color-surface-container-highest)',
      emptyLineBackground: 'var(--color-surface-container-low)',
      gutterColor: 'var(--color-text-tertiary)',
      addedGutterColor: 'var(--color-diff-added-text)',
      removedGutterColor: 'var(--color-diff-removed-text)',
      codeFoldContentColor: 'var(--color-text-tertiary)',
      diffViewerTitleBackground: 'var(--color-diff-title-bg)',
      diffViewerTitleColor: 'var(--color-diff-title-color)',
      diffViewerTitleBorderColor: 'var(--color-diff-title-border)',
    },
  },
  diffContainer: {
    borderRadius: '0',
    fontSize: '12px',
    lineHeight: '1.45',
    fontFamily: 'var(--font-mono)',
  },
  line: {
    padding: '1px 0',
  },
  gutter: {
    padding: '1px 8px',
    minWidth: '40px',
    fontSize: '11px',
  },
  wordDiff: {
    padding: '1px 2px',
    borderRadius: '2px',
  },
}

const HUGE_DIFF_LINES = 3000
const HUGE_DIFF_PREVIEW_LINES = 400

function DiffViewerImpl({ filePath, oldString, newString }: Props) {
  const language = inferLanguage(filePath)
  const [showFull, setShowFull] = useState(false)

  const oldLines = oldString.split('\n')
  const newLines = newString.split('\n')
  const { additions, deletions } = useMemo(() => {
    let added = 0
    let removed = 0
    for (const part of diffLines(oldString, newString)) {
      if (part.added) added += part.count ?? 0
      else if (part.removed) removed += part.count ?? 0
    }
    return { additions: added, deletions: removed }
  }, [oldString, newString])
  const showCounts = additions > 0 || deletions > 0

  const totalLines = oldLines.length + newLines.length
  const isLargeDiff =
    oldString.length + newString.length > 20000 || totalLines > 600
  const isHugeDiff = totalLines > HUGE_DIFF_LINES
  const truncate = isHugeDiff && !showFull
  const renderedOld = truncate
    ? oldLines.slice(0, HUGE_DIFF_PREVIEW_LINES).join('\n')
    : oldString
  const renderedNew = truncate
    ? newLines.slice(0, HUGE_DIFF_PREVIEW_LINES).join('\n')
    : newString

  return (
    <div className="overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-outline-variant)]/50 bg-[var(--color-surface-container-low)]">
      {}
      <div className="flex items-center justify-between border-b border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] px-3 py-1.5">
        <div className="min-w-0">
          <div className="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
            {filePath}
          </div>
          {showCounts && (
            <div className="mt-1 flex items-center gap-2 text-[10px] uppercase tracking-[0.14em]">
              {additions > 0 && (
                <span className="rounded-full bg-[var(--color-diff-added-bg)] px-2 py-0.5 text-[var(--color-diff-added-text)]">+{additions}</span>
              )}
              {deletions > 0 && (
                <span className="rounded-full bg-[var(--color-diff-removed-bg)] px-2 py-0.5 text-[var(--color-diff-removed-text)]">-{deletions}</span>
              )}
            </div>
          )}
        </div>
        <CopyButton
          text={`--- ${filePath}\n+++ ${filePath}`}
          label="Copy path"
          className="rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] px-2 py-1 text-[11px] text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
        />
      </div>

      {}
      <div className="max-h-[400px] overflow-auto">
        <ReactDiffViewer
          oldValue={renderedOld}
          newValue={renderedNew}
          splitView={false}
          compareMethod={isLargeDiff ? DiffMethod.LINES : DiffMethod.WORDS}
          renderContent={isLargeDiff ? undefined : (str) => highlightSyntax(str, language)}
          hideLineNumbers={false}
          styles={diffStyles}
          useDarkTheme={document.documentElement.getAttribute('data-theme') === 'dark'}
        />
      </div>
      {truncate && (
        <button
          type="button"
          onClick={() => setShowFull(true)}
          className="w-full border-t border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] px-3 py-1.5 text-[11px] text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-container-high)] hover:text-[var(--color-text-primary)]"
        >
          {`… ${totalLines - HUGE_DIFF_PREVIEW_LINES * 2} more lines — show full diff`}
        </button>
      )}
    </div>
  )
}

export const DiffViewer = memo(
  DiffViewerImpl,
  (prev, next) =>
    prev.filePath === next.filePath &&
    prev.oldString === next.oldString &&
    prev.newString === next.newString,
)
