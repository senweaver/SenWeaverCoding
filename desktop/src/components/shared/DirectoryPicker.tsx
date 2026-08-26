// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { sessionsApi, type RecentProject } from '../../api/sessions'
import { filesystemApi } from '../../api/filesystem'
import { useTranslation } from '../../i18n'
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown'
import {
  activateMinimalInputWindow,
  isMinimalInputWindow,
  setMinimalInputKeepVisible,
} from '../../lib/minimalMode'

type Props = {
  value: string
  onChange: (path: string) => void
}

type DirEntry = { name: string; path: string; isDirectory: boolean }

let cachedProjects: RecentProject[] | null = null
let cachedTotal = 0
let cacheTimestamp = 0
const CACHE_TTL = 30_000
const PAGE_SIZE = 10

function isTauriRuntime() {
  return typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
}

export function DirectoryPicker({ value, onChange }: Props) {
  const t = useTranslation()
  const [isOpen, setIsOpen] = useState(false)
  const [mode, setMode] = useState<'recent' | 'browse'>('recent')
  const [projects, setProjects] = useState<RecentProject[]>([])
  const [total, setTotal] = useState(0)
  const [loadingMore, setLoadingMore] = useState(false)
  const [browseEntries, setBrowseEntries] = useState<DirEntry[]>([])
  const [browsePath, setBrowsePath] = useState('')
  const [browseParent, setBrowseParent] = useState('')
  const [loading, setLoading] = useState(false)
  const { triggerRef, menuRef, style, portalTarget } = useAnchoredDropdown<HTMLButtonElement>(
    isOpen,
    () => setIsOpen(false),
    { estimatedHeight: 380, overflow: 'hidden' },
  )

  useEffect(() => {
    if (!isOpen || mode !== 'recent') return

    if (cachedProjects && Date.now() - cacheTimestamp < CACHE_TTL) {
      setProjects(cachedProjects)
      setTotal(cachedTotal)
      return
    }
    setLoading(true)
    sessionsApi.getRecentProjects({ limit: PAGE_SIZE, offset: 0 })
      .then(({ projects: p, total: t }) => {
        cachedProjects = p
        cachedTotal = t
        cacheTimestamp = Date.now()
        setProjects(p)
        setTotal(t)
      })
      .catch(() => { setProjects([]); setTotal(0) })
      .finally(() => setLoading(false))
  }, [isOpen, mode])

  const loadMore = async () => {
    if (loadingMore) return
    setLoadingMore(true)
    try {
      const { projects: more, total: t } = await sessionsApi.getRecentProjects({
        limit: PAGE_SIZE,
        offset: projects.length,
      })
      const merged = [...projects]
      const seen = new Set(merged.map((p) => p.realPath))
      for (const p of more) {
        if (!seen.has(p.realPath)) {
          merged.push(p)
          seen.add(p.realPath)
        }
      }
      cachedProjects = merged
      cachedTotal = t
      cacheTimestamp = Date.now()
      setProjects(merged)
      setTotal(t)
    } catch {
    } finally {
      setLoadingMore(false)
    }
  }

  const loadBrowseDir = async (path?: string) => {
    setLoading(true)
    try {
      const result = await filesystemApi.browse(path)
      setBrowsePath(result.currentPath)
      setBrowseParent(result.parentPath)
      setBrowseEntries(result.entries)
    } catch {  }
    setLoading(false)
  }

  const handleSelect = (path: string) => {
    onChange(path)
    setIsOpen(false)
    setMode('recent')

    cachedProjects = null
  }

  const handleChooseFolder = async () => {
    if (isTauriRuntime()) {

      setIsOpen(false)
      const holdInput = isMinimalInputWindow()
      try {
        if (holdInput) await setMinimalInputKeepVisible(true)
        const { open } = await import('@tauri-apps/plugin-dialog')
        const selected = await open({
          directory: true,
          multiple: false,
          title: t('dirPicker.chooseProjectFolder'),
        })
        const path = Array.isArray(selected) ? selected[0] : selected
        if (path && typeof path === 'string') onChange(path)
      } catch (err) {
        console.error('[DirectoryPicker] Failed to open folder dialog:', err)
      } finally {
        if (holdInput) {
          await activateMinimalInputWindow()
          await setMinimalInputKeepVisible(false)
        }
      }
    } else {

      setMode('browse')
      loadBrowseDir(value || undefined)
    }
  }

  const selectedProject = projects.find((p) => p.realPath === value)

  return (
    <div className="relative">
      {value ? (
        <button
          ref={triggerRef}
          onClick={() => { setIsOpen(!isOpen); setMode('recent') }}
          className="flex items-center gap-1.5 rounded-full bg-[var(--color-surface-container-low)] px-2.5 py-0.5 text-xs transition-colors hover:bg-[var(--color-surface-hover)]"
        >
          {selectedProject?.isGit ? (
            <svg width="13" height="13" viewBox="0 0 16 16" fill="currentColor" className="text-[var(--color-text-secondary)]">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
            </svg>
          ) : (
            <span className="material-symbols-outlined text-[12px] text-[var(--color-text-secondary)]">folder</span>
          )}
          <span className="font-medium text-[var(--color-text-primary)]">
            {selectedProject?.repoName || selectedProject?.projectName || value.split('/').pop()}
          </span>
          {selectedProject?.branch && (
            <>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className="text-[var(--color-text-tertiary)]">
                <circle cx="18" cy="18" r="3" /><circle cx="6" cy="6" r="3" />
                <path d="M13 6h3a2 2 0 0 1 2 2v7" /><line x1="6" y1="9" x2="6" y2="21" />
              </svg>
              <span className="text-[var(--color-text-tertiary)]">{selectedProject.branch}</span>
            </>
          )}
          <span className="material-symbols-outlined text-[12px] text-[var(--color-text-tertiary)]">expand_more</span>
        </button>
      ) : (
        <button
          ref={triggerRef}
          onClick={() => { setIsOpen(!isOpen); setMode('recent') }}
          className="flex items-center gap-1.5 text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
        >
          <span className="material-symbols-outlined text-[12px]">folder_open</span>
          {t('dirPicker.selectProject')}
        </button>
      )}

      {isOpen && style && createPortal(
        <div
          ref={menuRef}
          className="flex w-[400px] flex-col bg-[var(--color-surface-container-lowest)] border border-[var(--color-border)] rounded-xl shadow-[var(--shadow-dropdown)] overflow-hidden"
          style={style}
        >
          {mode === 'recent' ? (
            <>
              <div className="px-4 py-2 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
                {t('dirPicker.recent')}
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto">
                {loading ? (
                  <div className="px-4 py-6 text-center text-xs text-[var(--color-text-tertiary)]">{t('common.loading')}</div>
                ) : projects.length === 0 ? (
                  <div className="px-4 py-6 text-center text-xs text-[var(--color-text-tertiary)]">{t('dirPicker.noRecent')}</div>
                ) : (
                  projects.map((project) => {
                    const isSelected = project.realPath === value
                    return (
                      <button
                        key={project.projectPath}
                        onClick={() => handleSelect(project.realPath)}
                        className={`w-full flex items-center gap-3 px-4 py-2 text-left transition-colors hover:bg-[var(--color-surface-hover)] ${
                          isSelected ? 'bg-[var(--color-surface-selected)]' : ''
                        }`}
                      >
                        {project.isGit ? (
                          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--color-text-secondary)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className="flex-shrink-0">
                            <circle cx="18" cy="18" r="3" /><circle cx="6" cy="6" r="3" />
                            <path d="M13 6h3a2 2 0 0 1 2 2v7" /><line x1="6" y1="9" x2="6" y2="21" />
                          </svg>
                        ) : (
                          <span className="material-symbols-outlined text-[20px] text-[var(--color-text-secondary)] flex-shrink-0">folder</span>
                        )}
                        <div className="flex-1 min-w-0">
                          <div className="truncate text-xs font-semibold text-[var(--color-text-primary)]">
                            {project.repoName || project.projectName}
                          </div>
                          <div className="truncate font-[var(--font-mono)] text-xs text-[var(--color-text-tertiary)]">
                            {project.realPath}
                          </div>
                        </div>
                        {isSelected && (
                          <span className="material-symbols-outlined flex-shrink-0 text-[16px] text-[var(--color-brand)]" style={{ fontVariationSettings: "'FILL' 1" }}>
                            check
                          </span>
                        )}
                      </button>
                    )
                  })
                )}
                {!loading && projects.length > 0 && projects.length < total && (
                  <button
                    onClick={loadMore}
                    disabled={loadingMore}
                    className="w-full px-4 py-2.5 text-center text-xs font-medium text-[var(--color-text-accent)] transition-colors hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                  >
                    {loadingMore ? t('common.loading') : t('dirPicker.more')}
                  </button>
                )}
              </div>

              <div className="border-t border-[var(--color-border)]">
                <button
                  onClick={handleChooseFolder}
                  className="w-full flex items-center gap-3 px-4 py-2 text-left hover:bg-[var(--color-surface-hover)] transition-colors"
                >
                  <span className="material-symbols-outlined text-[20px] text-[var(--color-text-tertiary)]">create_new_folder</span>
                  <span className="text-xs text-[var(--color-text-tertiary)]">{t('dirPicker.chooseFolder')}</span>
                </button>
              </div>
            </>
          ) : (

            <>
              <div className="px-3 py-2 border-b border-[var(--color-border)] flex items-center gap-1 flex-wrap">
                <button onClick={() => setMode('recent')} className="text-xs text-[var(--color-text-accent)] hover:underline mr-2">
                  {'← ' + t('dirPicker.recent')}
                </button>
                <button onClick={() => loadBrowseDir('/')} className="text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]">/</button>
                {browsePath.split('/').filter(Boolean).map((seg, i, arr) => (
                  <span key={i} className="flex items-center gap-1">
                    <span className="text-xs text-[var(--color-text-tertiary)]">/</span>
                    <button
                      onClick={() => loadBrowseDir('/' + arr.slice(0, i + 1).join('/'))}
                      className="text-xs text-[var(--color-text-accent)] hover:underline"
                    >{seg}</button>
                  </span>
                ))}
              </div>

              <div className="min-h-0 flex-1 overflow-y-auto">
                {loading ? (
                  <div className="px-3 py-4 text-center text-xs text-[var(--color-text-tertiary)]">{t('common.loading')}</div>
                ) : (
                  <>
                    {browseParent && browseParent !== browsePath && (
                      <button onClick={() => loadBrowseDir(browseParent)} className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-[var(--color-surface-hover)]">
                        <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]">arrow_upward</span>
                        <span className="text-xs text-[var(--color-text-secondary)]">..</span>
                      </button>
                    )}
                    {browseEntries.length === 0 ? (
                      <div className="px-3 py-4 text-center text-xs text-[var(--color-text-tertiary)]">{t('dirPicker.noSubdirs')}</div>
                    ) : browseEntries.map((entry) => (
                      <button
                        key={entry.path}
                        className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-[var(--color-surface-hover)]"
                      >
                        <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]" onClick={() => loadBrowseDir(entry.path)}>folder</span>
                        <span className="text-xs text-[var(--color-text-primary)] flex-1" onClick={() => loadBrowseDir(entry.path)}>{entry.name}</span>
                        <button onClick={() => handleSelect(entry.path)} className="h-7 rounded-lg px-2.5 text-xs font-semibold text-[var(--color-brand)] transition-colors hover:bg-[var(--color-surface-hover)]">
                          {t('common.select')}
                        </button>
                      </button>
                    ))}
                  </>
                )}
              </div>

              <div className="px-3 py-2 border-t border-[var(--color-border)] flex justify-between items-center">
                <span className="truncate font-[var(--font-mono)] text-xs text-[var(--color-text-tertiary)]">{browsePath}</span>
                <button onClick={() => handleSelect(browsePath)} className="h-7 rounded-lg bg-[var(--color-brand)] px-3 text-xs font-semibold text-[var(--color-on-primary)] hover:opacity-90">
                  {t('dirPicker.useThisFolder')}
                </button>
              </div>
            </>
          )}
        </div>,
        portalTarget,
      )}
    </div>
  )
}
