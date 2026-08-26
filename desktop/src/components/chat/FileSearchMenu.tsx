// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { forwardRef, useState, useEffect, useRef, useCallback, useImperativeHandle, useMemo } from 'react'
import { ApiError } from '../../api/client'
import { filesystemApi } from '../../api/filesystem'
import { sessionsApi } from '../../api/sessions'
import { useTranslation } from '../../i18n'
import type { TranslationKey } from '../../i18n'
import type { SessionListItem } from '../../types/session'
import { resolveSessionTitle } from '../../utils/sessionTitle'

type DirEntry = {
  name: string
  path: string
  isDirectory: boolean
}

export type FileSearchMenuHandle = {
  handleKeyDown: (e: KeyboardEvent) => void
}

type Props = {
  cwd: string
  filter?: string
  currentSessionId?: string | null
  onSelect: (path: string, relativePath: string, isDir: boolean) => void
  onSelectSession?: (sessionId: string, title: string) => void
}

const SESSION_MATCH_LIMIT = 5

export const FileSearchMenu = forwardRef<FileSearchMenuHandle, Props>(
  ({ cwd, filter = '', currentSessionId = null, onSelect, onSelectSession }, ref) => {
  const t = useTranslation()
  const [entries, setEntries] = useState<DirEntry[]>([])
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [errorKey, setErrorKey] = useState<TranslationKey | null>(null)
  const [currentPath, setCurrentPath] = useState(cwd)
  const [loading, setLoading] = useState(false)
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [sessions, setSessions] = useState<SessionListItem[]>([])
  const listRef = useRef<HTMLDivElement>(null)
  const currentPathRef = useRef(cwd)

  const getErrorState = (error: unknown): { errorKey: TranslationKey | null; errorMessage: string | null } => {
    if (error instanceof ApiError) {
      if (error.status === 403) {
        return { errorKey: 'fileSearch.accessDenied', errorMessage: null }
      }

      const apiMessage =
        typeof error.body === 'string'
          ? error.body
          : typeof error.body === 'object' &&
              error.body !== null &&
              'error' in error.body &&
              typeof error.body.error === 'string'
            ? error.body.error
            : null

      if (apiMessage) {
        return { errorKey: null, errorMessage: apiMessage }
      }
    }

    return { errorKey: 'fileSearch.loadFailed', errorMessage: null }
  }

  const parseFilter = (rawFilter: string): { navigateTo: string; searchQuery: string } => {
    const base = currentPathRef.current
    if (!rawFilter || !rawFilter.includes('/')) {
      return { navigateTo: base, searchQuery: rawFilter }
    }
    const lastSlash = rawFilter.lastIndexOf('/')
    const dirPart = rawFilter.slice(0, lastSlash + 1)
    const searchPart = rawFilter.slice(lastSlash + 1)
    const navigateTo = dirPart === '' ? base : `${base}/${dirPart}`
    return { navigateTo, searchQuery: searchPart }
  }

  const loadDir = useCallback(async (dirPath: string, searchQuery: string) => {
    setLoading(true)
    setErrorMessage(null)
    setErrorKey(null)

    if (dirPath !== currentPathRef.current) {
      setCurrentPath(dirPath)
      currentPathRef.current = dirPath
    }
    try {
      if (searchQuery) {
        const result = await filesystemApi.search(searchQuery, dirPath)
        setEntries(result.entries)
      } else {
        const result = await filesystemApi.browse(dirPath, { includeFiles: true })
        setEntries(result.entries)
      }
      setSelectedIndex(0)
    } catch (error) {
      setEntries([])
      const nextError = getErrorState(error)
      setErrorKey(nextError.errorKey)
      setErrorMessage(nextError.errorMessage)
    }
    setLoading(false)
  }, [])

  useEffect(() => {
    currentPathRef.current = cwd
    const { navigateTo, searchQuery } = parseFilter(filter)
    void loadDir(navigateTo, searchQuery)
  }, [cwd, filter, loadDir])

  const sessionSourceEnabled = !!onSelectSession

  useEffect(() => {
    if (!sessionSourceEnabled) return
    let cancelled = false
    void sessionsApi
      .list({ limit: 100 })
      .then(({ sessions: items }) => {
        if (!cancelled) setSessions(items)
      })
      .catch(() => {
        if (!cancelled) setSessions([])
      })
    return () => {
      cancelled = true
    }
  }, [sessionSourceEnabled])

  const untitledLabel = t('sidebar.untitled')

  const visibleSessions = useMemo(() => {
    if (!sessionSourceEnabled) return []
    if (filter.includes('/')) return []
    const pool = sessions.filter((s) => s.id !== currentSessionId)
    const needle = filter.trim().toLowerCase()
    if (!needle) return pool.slice(0, SESSION_MATCH_LIMIT)
    return pool
      .filter((s) =>
        resolveSessionTitle(s.title, untitledLabel).toLowerCase().includes(needle),
      )
      .slice(0, SESSION_MATCH_LIMIT)
  }, [sessionSourceEnabled, sessions, currentSessionId, filter, untitledLabel])

  const dirs = entries.filter((e) => e.isDirectory)
  const files = entries.filter((e) => !e.isDirectory)
  const orderedEntries = useMemo(() => {
    return [...entries.filter((e) => e.isDirectory), ...entries.filter((e) => !e.isDirectory)]
  }, [entries])

  const sessionCount = visibleSessions.length
  const totalCount = sessionCount + orderedEntries.length

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIndex((prev) => Math.min(prev + 1, Math.max(totalCount - 1, 0)))
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIndex((prev) => Math.max(prev - 1, 0))
      return
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      if (selectedIndex < sessionCount) {
        const session = visibleSessions[selectedIndex]
        if (session && onSelectSession) {
          onSelectSession(session.id, resolveSessionTitle(session.title, untitledLabel))
        }
        return
      }
      const entry = orderedEntries[selectedIndex - sessionCount]
      if (entry) {
        onSelect(entry.path, entry.name, entry.isDirectory)
      }
      return
    }
  }, [
    orderedEntries,
    visibleSessions,
    sessionCount,
    totalCount,
    selectedIndex,
    untitledLabel,
    onSelect,
    onSelectSession,
  ])

  useImperativeHandle(ref, () => ({ handleKeyDown }), [handleKeyDown])

  useEffect(() => {
    const el = listRef.current?.querySelector(`[data-index="${selectedIndex}"]`) as HTMLButtonElement | null
    el?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex])

  const breadcrumbs: string[] = []
  if (currentPath !== cwd && currentPath.startsWith(cwd)) {
    const rel = currentPath.slice(cwd.length).replace(/^\//, '')
    if (rel) breadcrumbs.push(...rel.split('/'))
  }

  const formatSessionDate = (iso: string): string => {
    const ts = Date.parse(iso)
    if (Number.isNaN(ts)) return ''
    return new Date(ts).toLocaleDateString()
  }

  return (
    <div
      id="file-search-menu"
      className="absolute left-0 bottom-full mb-2 z-50 w-full min-w-0 overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]"
      onMouseDown={(e) => e.stopPropagation()}
    >
      {}
      <div className="flex items-center gap-1.5 border-b border-[var(--color-border)] px-3 py-2 text-[11px]">
        <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">folder_open</span>
        <span className="text-[var(--color-text-tertiary)] font-mono">{cwd.split('/').pop() || cwd}</span>
        {breadcrumbs.map((seg, i) => (
          <span key={i} className="flex items-center gap-1">
            <span className="text-[var(--color-text-tertiary)]">/</span>
            <span className="text-[var(--color-text-primary)] font-mono">{seg}</span>
          </span>
        ))}
        {loading && (
          <span className="material-symbols-outlined text-[12px] text-[var(--color-text-tertiary)] animate-spin ml-1">progress_activity</span>
        )}
        {currentPath !== cwd && currentPath.startsWith(cwd) && (
          <button
            type="button"
            onClick={() =>
              onSelect(currentPath, currentPath.split('/').pop() || currentPath, true)
            }
            className="ml-auto flex shrink-0 items-center gap-1 rounded border border-[var(--color-border)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent)] hover:bg-[var(--color-surface)] hover:text-[var(--color-text-primary)]"
            title={t('fileSearch.attachFolder')}
          >
            <span className="material-symbols-outlined text-[12px]">add</span>
            {t('fileSearch.attachFolder')}
          </button>
        )}
      </div>

      {}
      <div ref={listRef} className="max-h-[300px] overflow-y-auto py-1">
        {sessionCount > 0 && (
          <>
            <div className="px-3 pb-1 pt-1.5 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
              {t('fileSearch.sessions')}
            </div>
            {visibleSessions.map((session, i) => {
              const title = resolveSessionTitle(session.title, untitledLabel)
              return (
                <button
                  key={session.id}
                  data-index={i}
                  onClick={() => onSelectSession?.(session.id, title)}
                  onMouseEnter={() => setSelectedIndex(i)}
                  className={`w-full flex items-center gap-3 px-3 py-2 text-left transition-colors ${
                    selectedIndex === i ? 'bg-[var(--color-surface-hover)]' : 'hover:bg-[var(--color-surface-hover)]'
                  }`}
                >
                  <span className="material-symbols-outlined text-[16px] text-[var(--color-accent)]">forum</span>
                  <span className="min-w-0 flex-1 truncate text-sm text-[var(--color-text-primary)]">{title}</span>
                  <span className="shrink-0 text-[10px] text-[var(--color-text-tertiary)]">
                    {formatSessionDate(session.modifiedAt)}
                  </span>
                </button>
              )
            })}
            {(entries.length > 0 || errorKey || errorMessage) && (
              <div className="mx-3 my-1 border-t border-[var(--color-border)]/60" />
            )}
          </>
        )}
        {loading && entries.length === 0 ? (
          sessionCount === 0 ? (
            <div className="px-4 py-6 text-center text-xs text-[var(--color-text-tertiary)]">{t('fileSearch.searching')}</div>
          ) : null
        ) : (errorKey || errorMessage) ? (
          <div className="px-4 py-6 text-center text-xs text-[var(--color-error)]">
            {errorKey ? t(errorKey) : errorMessage}
          </div>
        ) : entries.length === 0 ? (
          sessionCount === 0 ? (
            <div className="px-4 py-6 text-center text-xs text-[var(--color-text-tertiary)]">
              {filter ? t('fileSearch.noMatch') : t('fileSearch.noFiles')}
            </div>
          ) : null
        ) : (
          <>
            {}
            {dirs.map((entry, i) => (
              <div
                key={entry.path}
                data-index={sessionCount + i}
                onMouseEnter={() => setSelectedIndex(sessionCount + i)}
                className={`group flex w-full items-center gap-3 px-3 py-2 transition-colors ${
                  selectedIndex === sessionCount + i ? 'bg-[var(--color-surface-hover)]' : 'hover:bg-[var(--color-surface-hover)]'
                }`}
              >
                <button
                  type="button"
                  onClick={() => {
                    void loadDir(entry.path, filter)
                  }}
                  className="flex min-w-0 flex-1 items-center gap-3 text-left"
                  title={t('fileSearch.navigate')}
                >
                  <span className="material-symbols-outlined text-[16px] text-[var(--color-brand)]">folder</span>
                  <span className="text-sm text-[var(--color-text-primary)] truncate">{entry.name}</span>
                </button>
                <button
                  type="button"
                  onClick={() => onSelect(entry.path, entry.name, true)}
                  className="flex shrink-0 items-center gap-1 rounded border border-[var(--color-border)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent)] hover:bg-[var(--color-surface)] hover:text-[var(--color-text-primary)]"
                  title={t('fileSearch.attach')}
                >
                  <span className="material-symbols-outlined text-[12px]">add</span>
                  {t('fileSearch.attach')}
                </button>
              </div>
            ))}

            {}
            {files.map((entry, i) => {
              const idx = sessionCount + dirs.length + i
              return (
                <button
                  key={entry.path}
                  data-index={idx}
                  onClick={() => onSelect(entry.path, entry.name, false)}
                  onMouseEnter={() => setSelectedIndex(idx)}
                  className={`w-full flex items-center gap-3 px-3 py-2 text-left transition-colors ${
                    selectedIndex === idx ? 'bg-[var(--color-surface-hover)]' : 'hover:bg-[var(--color-surface-hover)]'
                  }`}
                >
                  <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)]">description</span>
                  <span className="text-sm text-[var(--color-text-primary)] truncate">{entry.name}</span>
                </button>
              )
            })}
          </>
        )}
      </div>

      {}
      <div className="flex items-center gap-1.5 border-t border-[var(--color-border)] px-3 py-1.5 text-[10px] text-[var(--color-text-tertiary)]">
        <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-1 py-0.5 font-mono">↑↓</kbd>
        <span>{t('fileSearch.navigate')}</span>
        <kbd className="ml-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-1 py-0.5 font-mono">Enter</kbd>
        <span>{t('fileSearch.attach')}</span>
        <kbd className="ml-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-1 py-0.5 font-mono">Esc</kbd>
        <span>{t('fileSearch.close')}</span>
      </div>
    </div>
  )
})

FileSearchMenu.displayName = 'FileSearchMenu'
