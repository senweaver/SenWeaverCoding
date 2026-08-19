// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding

import { useEffect, useMemo, useRef, useState } from 'react'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import { useTranslation } from '../../i18n'
import type { TranslationKey } from '../../i18n/locales/en'
import { useUIStore } from '../../stores/uiStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import { hasServerForLanguage, lspBridge } from '../../lib/lspBridge'
import type { FileSearchHit } from '../../types/workspaceFile'

export type FinderMode = 'quick-open' | 'search-in-files' | 'workspace-symbol'

type Props = {
  mode: FinderMode
  workDir: string | null
  onClose: () => void
}

type SymbolHit = {
  name: string
  containerName?: string
  kind?: number
  uri: string
  range: { start: { line: number; character: number }; end: { line: number; character: number } }
}

const TITLES: Record<FinderMode, TranslationKey> = {
  'quick-open': 'finder.quickOpen.title',
  'search-in-files': 'finder.searchInFiles.title',
  'workspace-symbol': 'finder.workspaceSymbol.title',
}

const PLACEHOLDERS: Record<FinderMode, TranslationKey> = {
  'quick-open': 'finder.quickOpen.placeholder',
  'search-in-files': 'finder.searchInFiles.placeholder',
  'workspace-symbol': 'finder.workspaceSymbol.placeholder',
}

function uriToRel(uri: string, workDir: string): string | null {
  if (!uri || !uri.startsWith('file://')) return null
  let p = uri.slice('file://'.length)
  try {
    p = decodeURIComponent(p)
  } catch {
  }
  let abs = p
  if (/^\/[A-Za-z]:\//.test(abs)) abs = abs.slice(1)
  const normalize = (s: string) => s.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
  const normRoot = normalize(workDir) + '/'
  const normAbs = normalize(abs)
  if (!normAbs.startsWith(normRoot)) return null
  let rel = abs.slice(workDir.length)
  rel = rel.replace(/\\/g, '/').replace(/^\/+/, '')
  return rel
}

export function WorkspaceFinder({ mode, workDir, onClose }: Props) {
  const t = useTranslation()
  const inputRef = useRef<HTMLInputElement>(null)
  const [query, setQuery] = useState('')
  const [hits, setHits] = useState<FileSearchHit[]>([])
  const [symbolHits, setSymbolHits] = useState<SymbolHit[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [activeIndex, setActiveIndex] = useState(0)
  const requestId = useRef(0)
  const requestNavigation = useWorkspaceFilesStore((s) => s.requestNavigation)
  const selectFile = useWorkspaceFilesStore((s) => s.selectFile)
  const scopeFromStore = useUIStore((s) => s.workspaceFinderScopeDir)
  const [scopeDir, setScopeDir] = useState<string | null>(scopeFromStore)
  useEffect(() => {
    setScopeDir(scopeFromStore)
  }, [scopeFromStore, mode])

  const visibleHits = useMemo(() => {
    if (!scopeDir) return hits
    const prefix = `${scopeDir}/`
    return hits.filter(
      (hit) => hit.relPath === scopeDir || hit.relPath.startsWith(prefix),
    )
  }, [hits, scopeDir])

  useEffect(() => {
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [mode])

  useEffect(() => {
    if (!workDir) return
    const trimmed = query.trim()
    if (!trimmed) {
      setHits([])
      setSymbolHits([])
      setError(null)
      return
    }
    if (mode === 'workspace-symbol') {
      const id = ++requestId.current
      setLoading(true)
      const handle = window.setTimeout(async () => {
        try {
          const result = (await lspBridge.workspaceSymbol({ query: trimmed })) as
            | Array<{
                name: string
                containerName?: string
                kind?: number
                location: { uri: string; range: SymbolHit['range'] }
              }>
            | null
          if (requestId.current !== id) return
          if (!Array.isArray(result)) {
            setSymbolHits([])
            return
          }
          const supported = (() => {
            const langs = ['rust', 'typescript', 'javascript', 'python', 'go', 'c', 'cpp']
            return langs.some((l) => hasServerForLanguage(l))
          })()
          if (!supported) {
            setError(t('finder.workspaceSymbol.unavailable'))
          } else {
            setError(null)
          }
          setSymbolHits(
            result.slice(0, 200).map((s) => ({
              name: s.name,
              containerName: s.containerName,
              kind: s.kind,
              uri: s.location.uri,
              range: s.location.range,
            })),
          )
        } catch (err) {
          if (requestId.current !== id) return
          setError(err instanceof Error ? err.message : String(err))
          setSymbolHits([])
        } finally {
          if (requestId.current === id) setLoading(false)
        }
      }, 220)
      return () => window.clearTimeout(handle)
    }
    const id = ++requestId.current
    setLoading(true)
    const handle = window.setTimeout(async () => {
      try {
        const res = await workspaceFilesApi.search({
          root: workDir,
          query: trimmed,
          limit: 200,
          kind: mode === 'search-in-files' ? 'content' : 'name',
        })
        if (requestId.current !== id) return
        setError(null)
        setHits(res.results)
      } catch (err) {
        if (requestId.current !== id) return
        setError(err instanceof Error ? err.message : String(err))
        setHits([])
      } finally {
        if (requestId.current === id) setLoading(false)
      }
    }, 220)
    return () => window.clearTimeout(handle)
  }, [mode, query, t, workDir])

  useEffect(() => {
    setActiveIndex(0)
  }, [visibleHits, symbolHits, mode])

  const totalCount =
    mode === 'workspace-symbol' ? symbolHits.length : visibleHits.length

  const handleSelectFile = async (hit: FileSearchHit) => {
    onClose()
    if (hit.line !== undefined) {
      try {
        await requestNavigation(hit.relPath, hit.line, 0)
      } catch {
      }
      return
    }
    try {
      await selectFile(hit.relPath)
    } catch {
    }
  }

  const handleSelectSymbol = async (hit: SymbolHit) => {
    if (!workDir) return
    const rel = uriToRel(hit.uri, workDir)
    if (rel === null) return
    onClose()
    try {
      await requestNavigation(
        rel,
        hit.range.start.line,
        hit.range.start.character,
      )
    } catch {
    }
  }

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      onClose()
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      if (mode === 'workspace-symbol') {
        const hit = symbolHits[activeIndex]
        if (hit) void handleSelectSymbol(hit)
      } else {
        const hit = visibleHits[activeIndex]
        if (hit) void handleSelectFile(hit)
      }
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      if (totalCount <= 0) return
      setActiveIndex((idx) => {
        const next = idx + 1
        return next >= totalCount ? totalCount - 1 : next
      })
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      if (totalCount <= 0) return
      setActiveIndex((idx) => (idx <= 0 ? 0 : idx - 1))
    }
  }

  const summary = useMemo(() => {
    if (loading) return t('common.loading')
    if (error) return error
    if (!query.trim()) {
      if (mode === 'quick-open') return t('finder.quickOpen.empty')
      if (mode === 'search-in-files') return t('finder.searchInFiles.empty')
      return t('finder.workspaceSymbol.empty')
    }
    if (totalCount === 0) return t('files.searchEmpty')
    return null
  }, [error, loading, mode, query, t, totalCount])

  return (
    <div
      className="fixed inset-0 z-[400] flex items-start justify-center bg-black/30 px-4 pt-16"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="w-full max-w-2xl overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl">
        <div className="flex h-9 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 text-xs font-medium text-[var(--color-text-secondary)]">
          <span>{t(TITLES[mode])}</span>
          <span className="text-[10px] text-[var(--color-text-tertiary)]">{totalCount}</span>
        </div>
        <div className="px-3 py-2">
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder={t(PLACEHOLDERS[mode])}
            className="h-8 w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
          />
          {scopeDir && mode !== 'workspace-symbol' && (
            <div className="mt-1.5 flex items-center gap-1">
              <span className="flex items-center gap-1 rounded-full border border-[var(--color-border)] bg-[var(--color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--color-text-secondary)]">
                <span className="material-symbols-outlined text-[12px]">folder</span>
                <span className="max-w-[280px] truncate" title={scopeDir}>
                  {scopeDir}
                </span>
                <button
                  type="button"
                  onClick={() => setScopeDir(null)}
                  aria-label={t('finder.scopeClear')}
                  title={t('finder.scopeClear')}
                  className="flex h-3.5 w-3.5 items-center justify-center rounded-full text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
                >
                  <span className="material-symbols-outlined text-[11px]">close</span>
                </button>
              </span>
            </div>
          )}
        </div>
        <div className="max-h-[420px] overflow-y-auto border-t border-[var(--color-border)]">
          {summary && (
            <div className="px-3 py-2 text-[11px] text-[var(--color-text-tertiary)]">
              {summary}
            </div>
          )}
          {!summary && mode !== 'workspace-symbol' &&
            visibleHits.map((hit, i) => (
              <button
                key={`${hit.relPath}:${hit.line ?? -1}:${i}`}
                type="button"
                onMouseEnter={() => setActiveIndex(i)}
                onClick={() => void handleSelectFile(hit)}
                className={`flex w-full flex-col items-start px-3 py-1.5 text-left text-xs hover:bg-[var(--color-surface-hover)] ${
                  i === activeIndex ? 'bg-[var(--color-surface-hover)]' : ''
                }`}
              >
                <span className="flex w-full items-center gap-2">
                  <span className="truncate font-medium text-[var(--color-text-primary)]">
                    {hit.name}
                  </span>
                  <span className="ml-auto truncate text-[10px] text-[var(--color-text-tertiary)]">
                    {hit.relPath}
                    {hit.line !== undefined ? `:${hit.line + 1}` : ''}
                  </span>
                </span>
                {hit.preview && (
                  <span className="mt-0.5 line-clamp-1 break-all font-mono text-[10px] text-[var(--color-text-tertiary)]">
                    {hit.preview}
                  </span>
                )}
              </button>
            ))}
          {!summary && mode === 'workspace-symbol' &&
            symbolHits.map((hit, i) => (
              <button
                key={`${hit.uri}:${hit.range.start.line}:${i}`}
                type="button"
                onMouseEnter={() => setActiveIndex(i)}
                onClick={() => void handleSelectSymbol(hit)}
                className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-[var(--color-surface-hover)] ${
                  i === activeIndex ? 'bg-[var(--color-surface-hover)]' : ''
                }`}
              >
                <span className="truncate font-medium text-[var(--color-text-primary)]">
                  {hit.name}
                </span>
                {hit.containerName && (
                  <span className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                    {hit.containerName}
                  </span>
                )}
                <span className="ml-auto truncate text-[10px] text-[var(--color-text-tertiary)]">
                  {workDir ? uriToRel(hit.uri, workDir) ?? hit.uri : hit.uri}
                  :{hit.range.start.line + 1}
                </span>
              </button>
            ))}
        </div>
      </div>
    </div>
  )
}
