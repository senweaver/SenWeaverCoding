// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import type { FileSearchHit } from '../../types/workspaceFile'

type Props = {
  workDir: string
  onSelect: (hit: FileSearchHit) => void
}

const SHORT_QUERY_DEBOUNCE = 250

export function WorkspaceSearchBar({ workDir, onSelect }: Props) {
  const t = useTranslation()
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<FileSearchHit[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [kind, setKind] = useState<'name' | 'content'>('name')
  const requestId = useRef(0)

  const trimmed = query.trim()

  useEffect(() => {
    setError(null)
    if (!trimmed) {
      setResults([])
      setLoading(false)
      return
    }
    const id = ++requestId.current
    setLoading(true)
    const handle = window.setTimeout(async () => {
      try {
        const res = await workspaceFilesApi.search({
          root: workDir,
          query: trimmed,
          limit: 200,
          kind,
        })
        if (requestId.current !== id) return
        setResults(res.results)
      } catch (err) {
        if (requestId.current !== id) return
        setError(err instanceof Error ? err.message : String(err))
        setResults([])
      } finally {
        if (requestId.current === id) setLoading(false)
      }
    }, SHORT_QUERY_DEBOUNCE)
    return () => window.clearTimeout(handle)
  }, [kind, trimmed, workDir])

  const showResults = open && trimmed.length > 0

  const summary = useMemo(() => {
    if (loading) return t('common.loading')
    if (error) return error
    if (trimmed.length === 0) return t('files.searchHint')
    if (results.length === 0) return t('files.searchEmpty')
    return null
  }, [error, loading, results.length, t, trimmed.length])

  return (
    <div className="relative px-2 py-1">
      <div className="relative flex items-center gap-1">
        <span
          aria-hidden="true"
          className="material-symbols-outlined absolute left-2 text-[14px] text-[var(--color-text-tertiary)]"
        >
          search
        </span>
        <input
          type="text"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value)
            setOpen(true)
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => window.setTimeout(() => setOpen(false), 120)}
          placeholder={t('files.searchPlaceholder')}
          className="h-7 w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] pl-7 pr-7 text-xs text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-accent)]"
        />
        {query && (
          <button
            type="button"
            onClick={() => setQuery('')}
            aria-label={t('common.cancel')}
            className="absolute right-1 flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]"
          >
            <span className="material-symbols-outlined text-[14px]">close</span>
          </button>
        )}
      </div>
      <div className="mt-1 flex items-center gap-1 text-[10px] text-[var(--color-text-tertiary)]">
        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setKind('name')}
          className={`rounded px-1.5 py-0.5 transition-colors ${
            kind === 'name'
              ? 'bg-[var(--color-surface)] text-[var(--color-text-secondary)]'
              : 'opacity-60 hover:opacity-90'
          }`}
          title={t('files.search.kindName')}
        >
          {t('files.search.kindName')}
        </button>
        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setKind('content')}
          className={`rounded px-1.5 py-0.5 transition-colors ${
            kind === 'content'
              ? 'bg-[var(--color-surface)] text-[var(--color-text-secondary)]'
              : 'opacity-60 hover:opacity-90'
          }`}
          title={t('files.search.kindContent')}
        >
          {t('files.search.kindContent')}
        </button>
      </div>
      {showResults && (
        <div className="absolute left-2 right-2 top-full z-30 mt-1 max-h-64 overflow-y-auto rounded border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-lg">
          {summary && (
            <div className="px-2 py-1 text-[11px] text-[var(--color-text-tertiary)]">
              {summary}
            </div>
          )}
          {!summary &&
            results.map((hit, idx) => (
              <button
                key={`${hit.relPath}::${hit.line ?? 0}::${idx}`}
                type="button"
                onMouseDown={(event) => {
                  event.preventDefault()
                  onSelect(hit)
                  setOpen(false)
                }}
                className="flex w-full flex-col items-start px-2 py-1 text-left text-xs hover:bg-[var(--color-surface-hover)]"
              >
                <span className="flex w-full items-center gap-1">
                  <span className="truncate font-medium text-[var(--color-text-primary)]">
                    {hit.name}
                  </span>
                  {kind === 'content' && hit.line !== undefined && (
                    <span className="ml-auto text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
                      L{hit.line}
                    </span>
                  )}
                </span>
                <span className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                  {hit.relPath}
                </span>
                {kind === 'content' && hit.preview && (
                  <span className="mt-0.5 line-clamp-1 w-full truncate font-mono text-[10px] text-[var(--color-text-secondary)]">
                    {hit.preview}
                  </span>
                )}
              </button>
            ))}
        </div>
      )}
    </div>
  )
}
