// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import { useUIStore } from '../../stores/uiStore'
import { hasServerForLanguage, lspBridge } from '../../lib/lspBridge'
import { workspaceAbsPathToUri, joinWorkspaceAbsPath } from '../../lib/workspacePath'
import { inferLanguageFromPath } from '../../lib/extLanguage'

function cheapHash(input: string): string {
  let h = 0x811c9dc5
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i)
    h = (h + ((h << 1) + (h << 4) + (h << 7) + (h << 8) + (h << 24))) >>> 0
  }
  return `${h.toString(16)}:${input.length}`
}

type LspPosition = { line: number; character: number }
type LspRange = { start: LspPosition; end: LspPosition }

type LspSymbol = {
  name: string
  kind: number
  detail?: string
  containerName?: string
  range?: LspRange
  selectionRange?: LspRange
  location?: { uri: string; range: LspRange }
  children?: LspSymbol[]
}

type FlatSymbol = {
  name: string
  detail?: string
  kind: number
  depth: number
  selectionRange: LspRange
  fullRange: LspRange
}

type SymbolBadge = { label: string; bg: string; fg: string }

const SYMBOL_KIND_BADGE: Record<number, SymbolBadge> = {
  1: { label: 'F', bg: '#7a7a7a', fg: '#ffffff' },
  2: { label: 'M', bg: '#9b6dff', fg: '#ffffff' },
  3: { label: 'N', bg: '#9b6dff', fg: '#ffffff' },
  4: { label: 'P', bg: '#a87a3a', fg: '#ffffff' },
  5: { label: 'C', bg: '#e9a23a', fg: '#1f1f1f' },
  6: { label: 'm', bg: '#b888ff', fg: '#ffffff' },
  7: { label: 'p', bg: '#5aa9f0', fg: '#ffffff' },
  8: { label: 'f', bg: '#5aa9f0', fg: '#ffffff' },
  9: { label: 'C+', bg: '#ec6aa0', fg: '#ffffff' },
  10: { label: 'E', bg: '#d96363', fg: '#ffffff' },
  11: { label: 'I', bg: '#3fb6c0', fg: '#ffffff' },
  12: { label: 'ƒ', bg: '#b888ff', fg: '#ffffff' },
  13: { label: 'v', bg: '#7a7a7a', fg: '#ffffff' },
  14: { label: 'K', bg: '#d96363', fg: '#ffffff' },
  15: { label: 'S', bg: '#5aa86e', fg: '#ffffff' },
  16: { label: '#', bg: '#3fb6c0', fg: '#ffffff' },
  17: { label: 'B', bg: '#5aa9f0', fg: '#ffffff' },
  18: { label: '[]', bg: '#c477c8', fg: '#ffffff' },
  19: { label: '{}', bg: '#e9a23a', fg: '#1f1f1f' },
  20: { label: 'K', bg: '#e3c25a', fg: '#1f1f1f' },
  21: { label: '\u2205', bg: '#7a7a7a', fg: '#ffffff' },
  22: { label: 'e', bg: '#d96363', fg: '#ffffff' },
  23: { label: 'S', bg: '#e9a23a', fg: '#1f1f1f' },
  24: { label: '\u03bb', bg: '#d96363', fg: '#ffffff' },
  25: { label: '+', bg: '#7a7a7a', fg: '#ffffff' },
  26: { label: 'T', bg: '#5aa9f0', fg: '#ffffff' },
}

const FALLBACK_BADGE: SymbolBadge = { label: '?', bg: '#5a5a5a', fg: '#ffffff' }

function SymbolKindBadge({ kind, active }: { kind: number; active: boolean }) {
  const badge = SYMBOL_KIND_BADGE[kind] ?? FALLBACK_BADGE
  return (
    <span
      aria-hidden="true"
      className="inline-flex h-3.5 w-3.5 flex-shrink-0 items-center justify-center rounded-[3px] text-[8px] font-semibold leading-none"
      style={{
        backgroundColor: badge.bg,
        color: badge.fg,
        opacity: active ? 1 : 0.92,
        fontFamily:
          '"Inter", "Segoe UI", system-ui, -apple-system, "Helvetica Neue", Arial, sans-serif',
      }}
    >
      {badge.label}
    </span>
  )
}

function flattenSymbols(
  list: LspSymbol[] | undefined,
  depth: number,
  out: FlatSymbol[],
) {
  if (!Array.isArray(list)) return
  for (const sym of list) {
    const selection =
      sym.selectionRange ??
      sym.range ??
      sym.location?.range ??
      null
    if (!selection?.start || !selection?.end) continue
    const full = sym.range ?? sym.location?.range ?? selection
    out.push({
      name: sym.name ?? '',
      detail: sym.detail,
      kind: sym.kind ?? 13,
      depth,
      selectionRange: selection,
      fullRange: full,
    })
    if (Array.isArray(sym.children) && sym.children.length > 0) {
      flattenSymbols(sym.children, depth + 1, out)
    }
  }
}

function parseMarkdownHeadings(text: string): FlatSymbol[] {
  if (!text) return []
  const lines = text.split('\n')
  const out: FlatSymbol[] = []
  let inFence = false
  let fenceMarker: string | null = null
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i] ?? ''
    const trimmed = raw.trimStart()
    if (trimmed.startsWith('```') || trimmed.startsWith('~~~')) {
      const marker = trimmed.startsWith('```') ? '```' : '~~~'
      if (!inFence) {
        inFence = true
        fenceMarker = marker
      } else if (fenceMarker === marker) {
        inFence = false
        fenceMarker = null
      }
      continue
    }
    if (inFence) continue
    const m = trimmed.match(/^(#{1,6})\s+(.+?)\s*#*\s*$/)
    if (!m || !m[1] || !m[2]) continue
    const level = m[1].length
    const heading = m[2].trim()
    if (!heading) continue
    const start: LspPosition = { line: i, character: raw.indexOf('#') >= 0 ? raw.indexOf('#') : 0 }
    const end: LspPosition = { line: i, character: raw.length }
    out.push({
      name: heading,
      detail: undefined,
      kind: 15,
      depth: Math.max(0, level - 1),
      selectionRange: { start, end },
      fullRange: { start, end },
    })
  }
  if (out.length === 0) return out
  for (let i = 0; i < out.length; i++) {
    const cur = out[i]
    if (!cur) continue
    let endLine = lines.length > 0 ? lines.length - 1 : cur.fullRange.end.line
    for (let j = i + 1; j < out.length; j++) {
      const next = out[j]
      if (next && next.depth <= cur.depth) {
        endLine = Math.max(next.fullRange.start.line - 1, cur.fullRange.start.line)
        break
      }
    }
    const endChar = lines[endLine]?.length ?? cur.fullRange.end.character
    cur.fullRange = {
      start: cur.fullRange.start,
      end: { line: endLine, character: endChar },
    }
  }
  return out
}

function isCursorInsideRange(line: number, column: number, range: LspRange): boolean {
  const { start, end } = range
  if (line < start.line || line > end.line) return false
  if (line === start.line && column < start.character) return false
  if (line === end.line && column > end.character) return false
  return true
}

type Props = {
  workDir: string
  onJump: (relPath: string, position: LspPosition) => void
}

export function OutlinePanel({ workDir, onJump }: Props) {
  const t = useTranslation()
  const activeTab = useWorkspaceFilesStore((s) => s.activeTab)
  const buffer = useWorkspaceFilesStore((s) => {
    if (!s.root || !s.activeTab) return undefined
    return s.files[`${s.root}::${s.activeTab}`]
  })
  const editorCursor = useUIStore((s) => s.editorCursor)

  const [symbols, setSymbols] = useState<FlatSymbol[]>([])
  const [loading, setLoading] = useState(false)
  const [collapsed, setCollapsed] = useState(false)

  const requestId = useRef(0)
  const lastRequestKey = useRef<string>('')

  const languageId = useMemo(() => {
    if (!activeTab) return null
    return inferLanguageFromPath(activeTab) ?? null
  }, [activeTab])

  const supported = languageId ? hasServerForLanguage(languageId) : false
  const isMarkdownFallback = languageId === 'markdown' && !supported

  const activeSymbolIndex = useMemo(() => {
    if (!editorCursor || !activeTab) return -1
    if (editorCursor.relPath !== activeTab) return -1
    const monacoLine = editorCursor.line
    const monacoCol = editorCursor.column
    const line = monacoLine - 1
    const column = Math.max(0, monacoCol - 1)
    let bestIdx = -1
    let bestDepth = -1
    for (let i = 0; i < symbols.length; i++) {
      const sym = symbols[i]
      if (!sym) continue
      if (isCursorInsideRange(line, column, sym.fullRange) && sym.depth > bestDepth) {
        bestDepth = sym.depth
        bestIdx = i
      }
    }
    return bestIdx
  }, [editorCursor, activeTab, symbols])

  useEffect(() => {
    if (!activeTab || !buffer || buffer.isBinary) {
      setSymbols([])
      return
    }
    const text = buffer.draft ?? ''
    const key = `${activeTab}::${cheapHash(text)}::${supported ? 'lsp' : isMarkdownFallback ? 'md' : 'none'}`

    if (isMarkdownFallback) {
      if (lastRequestKey.current === key) return
      lastRequestKey.current = key
      setSymbols(parseMarkdownHeadings(text))
      setLoading(false)
      return
    }
    if (!supported) {
      setSymbols([])
      return
    }
    if (lastRequestKey.current === key && symbols.length > 0) return
    lastRequestKey.current = key

    const id = ++requestId.current
    setLoading(true)
    const handle = window.setTimeout(async () => {
      try {
        const abs = joinWorkspaceAbsPath(workDir, activeTab)
        const uri = workspaceAbsPathToUri(abs)
        const result = (await lspBridge.documentSymbol({
          uri,
          languageId: languageId ?? undefined,
          text,
        })) as LspSymbol[] | null
        if (requestId.current !== id) return
        const flat: FlatSymbol[] = []
        if (Array.isArray(result)) {
          flattenSymbols(result, 0, flat)
        }
        setSymbols(flat)
      } catch {
        if (requestId.current !== id) return
        setSymbols([])
      } finally {
        if (requestId.current === id) setLoading(false)
      }
    }, 300)

    return () => window.clearTimeout(handle)
  }, [activeTab, buffer, isMarkdownFallback, languageId, supported, symbols.length, workDir])

  return (
    <div className="flex flex-shrink-0 flex-col border-t border-[var(--color-border)]">
      <button
        type="button"
        onClick={() => setCollapsed((c) => !c)}
        className="group sticky top-0 z-[8] flex h-7 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
      >
        <span className="flex items-center gap-1">
          <span className="material-symbols-outlined text-[14px]">
            {collapsed ? 'chevron_right' : 'expand_more'}
          </span>
          {t('files.outline.title')}
        </span>
        <span className="text-[10px] tabular-nums text-[var(--color-text-tertiary)]/70">
          {symbols.length > 0 ? symbols.length : ''}
        </span>
      </button>
      {!collapsed && (
        <div className="max-h-[260px] overflow-y-auto">
          {!activeTab && (
            <div className="px-3 py-2 text-[11px] text-[var(--color-text-tertiary)]">
              {t('files.noFileSelected')}
            </div>
          )}
          {activeTab && !supported && !isMarkdownFallback && (
            <div className="px-3 py-2 text-[11px] text-[var(--color-text-tertiary)] italic">
              {t('files.outline.unavailable')}
            </div>
          )}
          {activeTab && (supported || isMarkdownFallback) && loading && symbols.length === 0 && (
            <div className="px-3 py-2 text-[11px] text-[var(--color-text-tertiary)]">
              {t('files.outline.loading')}
            </div>
          )}
          {activeTab && (supported || isMarkdownFallback) && !loading && symbols.length === 0 && (
            <div className="px-3 py-2 text-[11px] text-[var(--color-text-tertiary)] italic">
              {t('files.outline.empty')}
            </div>
          )}
          {symbols.map((sym, i) => {
            const isActive = i === activeSymbolIndex
            return (
              <button
                key={`${sym.name}-${i}`}
                type="button"
                onClick={() => activeTab && onJump(activeTab, sym.selectionRange.start)}
                className={`flex w-full items-center gap-1.5 px-2 py-1 text-left text-[11px] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] ${
                  isActive
                    ? 'border-l-2 border-[var(--color-brand)] bg-[var(--color-brand)]/10 text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-secondary)]'
                }`}
                style={{ paddingLeft: `${sym.depth * 12 + 8}px` }}
              >
                <SymbolKindBadge kind={sym.kind} active={isActive} />
                <span className="truncate">{sym.name}</span>
                {sym.detail && (
                  <span className="ml-auto truncate text-[10px] text-[var(--color-text-tertiary)]/70">
                    {sym.detail}
                  </span>
                )}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}
