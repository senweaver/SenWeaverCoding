// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useSkillStore } from '../../stores/skillStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useTabStore } from '../../stores/tabStore'
import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'
import { useDockSuspend } from '../../hooks/useDockSuspend'
import { revealInExplorer } from '../../lib/revealInExplorer'
import type { SkillMeta, SkillSource } from '../../types/skill'
import { CreateSkillDialog } from './CreateSkillDialog'
import {
  skillsApi,
  type SkillInstallMode,
  type SkillInstallReport,
} from '../../api/skills'

const SOURCE_ORDER: SkillSource[] = ['user', 'project', 'plugin', 'mcp', 'bundled']

const SOURCE_ICONS: Record<SkillSource, string> = {
  user: 'person',
  project: 'folder',
  plugin: 'extension',
  mcp: 'hub',
  bundled: 'inventory_2',
}

const SOURCE_ACCENT_CLASSES: Record<SkillSource, string> = {
  user: 'bg-[var(--color-primary-fixed)] text-[var(--color-brand)]',
  project: 'bg-[var(--color-success-container)] text-[var(--color-success)]',
  plugin: 'bg-[var(--color-warning-container)] text-[var(--color-warning)]',
  mcp: 'bg-[var(--color-info-container)] text-[var(--color-info)]',
  bundled: 'bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)]',
}

function estimateTokens(contentLength: number) {
  return Math.ceil(contentLength / 4)
}

export function SkillList() {
  const { skills, isLoading, error, fetchSkills, fetchSkillDetail } =
    useSkillStore()
  const userSkillsDir = useSkillStore((s) => s.userSkillsDir)
  const deleteUserSkill = useSkillStore((s) => s.deleteUserSkill)
  const sessions = useSessionStore((s) => s.sessions)
  const activeSessionId = useTabStore((s) => s.activeTabId)
  const addToast = useUIStore((s) => s.addToast)
  const t = useTranslation()
  const activeSession = sessions.find((session) => session.id === activeSessionId)
  const currentWorkDir = activeSession?.workDir || undefined

  const [createOpen, setCreateOpen] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<SkillMeta | null>(null)
  useDockSuspend(pendingDelete !== null)
  const [isDragOver, setIsDragOver] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [conflictReports, setConflictReports] = useState<SkillInstallReport[]>([])
  useDockSuspend(conflictReports.length > 0)
  const dropZoneRef = useRef<HTMLDivElement | null>(null)
  const installingRef = useRef(false)
  const conflictOpenRef = useRef(false)

  useEffect(() => {
    installingRef.current = installing
  }, [installing])
  useEffect(() => {
    conflictOpenRef.current = conflictReports.length > 0
  }, [conflictReports])

  useEffect(() => {
    fetchSkills(currentWorkDir)
  }, [fetchSkills, currentWorkDir])

  useEffect(() => {
    let unlisten: (() => void) | null = null
    let cancelled = false
    void import('@tauri-apps/api/webview').then(async ({ getCurrentWebview }) => {
      try {
        const webview = getCurrentWebview()
        const off = await webview.onDragDropEvent((event) => {
          const payload = event.payload as
            | { type: 'enter'; paths: string[]; position: { x: number; y: number } }
            | { type: 'over'; position: { x: number; y: number } }
            | { type: 'drop'; paths: string[]; position: { x: number; y: number } }
            | { type: 'leave' }
          const dropZone = dropZoneRef.current
          if (!dropZone) return
          if (installingRef.current || conflictOpenRef.current) {
            if (payload.type === 'enter' || payload.type === 'over' || payload.type === 'drop') {
              setIsDragOver(false)
            }
            return
          }
          if (payload.type === 'enter' || payload.type === 'over') {
            const inside = isPointInsideElement(
              dropZone,
              payload.position.x,
              payload.position.y,
            )
            setIsDragOver(inside)
          } else if (payload.type === 'leave') {
            setIsDragOver(false)
          } else if (payload.type === 'drop') {
            const inside = isPointInsideElement(
              dropZone,
              payload.position.x,
              payload.position.y,
            )
            setIsDragOver(false)
            if (inside && payload.paths && payload.paths.length > 0) {
              void runInstall(payload.paths, 'abort')
            }
          }
        })
        if (cancelled) {
          off()
        } else {
          unlisten = off
        }
      } catch {
        // noop: not running inside Tauri webview
      }
    })
    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }    
  }, [])

  async function runInstall(sources: string[], mode: SkillInstallMode = 'abort') {
    if (sources.length === 0) return
    setInstalling(true)
    try {
      const { results } = await skillsApi.installUserSkills(sources, mode)
      const installed = results.filter(
        (r) =>
          r.status === 'installed' ||
          r.status === 'overwritten' ||
          r.status === 'renamed',
      )
      const conflicts = results.filter((r) => r.status === 'exists')
      const duplicates = results.filter((r) => r.status === 'duplicate')
      const errors = results.filter((r) => r.status === 'error')
      const renamed = results.filter((r) => r.status === 'renamed')

      if (installed.length > 0) {
        addToast({
          type: 'success',
          message: t('settings.skills.installSummary', {
            count: String(installed.length),
          }),
        })
        await fetchSkills(currentWorkDir)
      }
      for (const r of renamed) {
        addToast({
          type: 'info',
          message: t('settings.skills.renamedToast', {
            name: r.name ?? '',
          }),
        })
      }
      for (const err of errors) {
        addToast({
          type: 'error',
          message: `${err.source}: ${err.error ?? 'install failed'}`,
        })
      }
      const pending = [...conflicts, ...duplicates]
      if (pending.length > 0) {
        setConflictReports(pending)
      }
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.skills.installFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setInstalling(false)
    }
  }

  async function handleConflictResolve(mode: SkillInstallMode) {
    const sources = conflictReports.map((c) => c.source)
    setConflictReports([])
    await runInstall(sources, mode)
  }

  async function handleOpenSkillsFolder() {
    if (!userSkillsDir) return
    try {
      await revealInExplorer(userSkillsDir)
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.skills.openFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }

  async function handleConfirmDeleteSkill() {
    if (!pendingDelete) return
    const target = pendingDelete
    setPendingDelete(null)
    try {
      await deleteUserSkill(target.name)
      addToast({ type: 'success', message: t('settings.skills.deletedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.skills.deleteFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }

  const grouped = useMemo(() => {
    const result: Partial<Record<SkillSource, SkillMeta[]>> = {}
    for (const skill of skills) {
      const src = skill.source as SkillSource
      ;(result[src] ??= []).push(skill)
    }
    return result
  }, [skills])

  const totalTokens = useMemo(
    () => skills.reduce((sum, skill) => sum + estimateTokens(skill.contentLength), 0),
    [skills],
  )

  const visibleGroupCount = useMemo(
    () => SOURCE_ORDER.filter((source) => (grouped[source] ?? []).length > 0).length,
    [grouped],
  )

  if (isLoading) {
    return (
      <div className="flex justify-center py-12">
        <div className="animate-spin w-5 h-5 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
      </div>
    )
  }

  if (error) {
    return <div className="text-xs text-[var(--color-error)] py-4">{error}</div>
  }

  if (skills.length === 0) {
    return (
      <div ref={dropZoneRef} className="relative space-y-3">
        <div className="flex items-center justify-end gap-2">
          <button
            onClick={() => setCreateOpen(true)}
            disabled={!userSkillsDir}
            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] bg-[var(--color-brand)] text-white hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <span className="material-symbols-outlined text-[14px]">add</span>
            {t('settings.skills.newButton')}
          </button>
          {userSkillsDir && (
            <button
              onClick={() => void handleOpenSkillsFolder()}
              className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]"
            >
              <span className="material-symbols-outlined text-[14px]">folder_open</span>
              {t('settings.skills.openDirectory')}
            </button>
          )}
        </div>
        <div className="text-center py-10 rounded-2xl border border-dashed border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-6">
          <span className="material-symbols-outlined text-[32px] text-[var(--color-text-tertiary)] mb-2 block">
            auto_awesome
          </span>
          <p className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.skills.empty')}
          </p>
          <p className="text-xs text-[var(--color-text-tertiary)] mt-1">
            {t('settings.skills.emptyHint')}
          </p>
          <p className="text-xs text-[var(--color-text-secondary)] mt-3">
            {t('settings.skills.dropHint')}
          </p>
          {userSkillsDir && (
            <p className="text-xs text-[var(--color-text-tertiary)] mt-2 font-mono break-all">
              {userSkillsDir}
            </p>
          )}
        </div>
        <CreateSkillDialog
          open={createOpen}
          onClose={() => setCreateOpen(false)}
          onCreated={() => setCreateOpen(false)}
        />
        <DropOverlay
          visible={isDragOver}
          installing={installing}
          title={t('settings.skills.dropOverlayTitle')}
          hint={t('settings.skills.dropOverlayHint')}
          installingLabel={t('settings.skills.installing')}
        />
        {conflictReports.length > 0 && (
          <ConflictDialog
            reports={conflictReports}
            onCancel={() => setConflictReports([])}
            onOverwrite={() => void handleConflictResolve('overwrite')}
            onKeepBoth={() => void handleConflictResolve('rename')}
            title={
              conflictReports.length > 1
                ? t('settings.skills.conflictTitlePlural')
                : t('settings.skills.conflictTitle')
            }
            body={t('settings.skills.conflictBody')}
            cancelLabel={t('common.cancel')}
            overwriteLabel={t('settings.skills.conflictOverwrite')}
            keepBothLabel={t('settings.skills.conflictKeepBoth')}
          />
        )}
      </div>
    )
  }

  return (
    <div ref={dropZoneRef} className="relative flex flex-col gap-4 min-w-0">
      <section className="rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] overflow-hidden">
        <div className="grid gap-4 px-4 py-4 min-w-0 xl:grid-cols-[minmax(0,1.6fr)_minmax(320px,1fr)] xl:items-end">
          <div className="min-w-0">
            <div className="text-xs font-semibold uppercase tracking-[0.2em] text-[var(--color-text-tertiary)] mb-2">
              {t('settings.skills.browserEyebrow')}
            </div>
            <div className="flex items-center gap-3 mb-2">
              <span className="material-symbols-outlined text-[18px] text-[var(--color-brand)]">
                auto_awesome
              </span>
              <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
                {t('settings.skills.browserTitle')}
              </h3>
              <div className="ml-auto flex items-center gap-2">
                <button
                  onClick={() => setCreateOpen(true)}
                  disabled={!userSkillsDir}
                  className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] bg-[var(--color-brand)] text-white hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  <span className="material-symbols-outlined text-[14px]">add</span>
                  {t('settings.skills.newButton')}
                </button>
                {userSkillsDir && (
                  <button
                    onClick={() => void handleOpenSkillsFolder()}
                    title={userSkillsDir}
                    className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]"
                  >
                    <span className="material-symbols-outlined text-[14px]">folder_open</span>
                    {t('settings.skills.openDirectory')}
                  </button>
                )}
              </div>
            </div>
            <p className="text-xs leading-5 text-[var(--color-text-secondary)] max-w-3xl">
              {t('settings.skills.browserDescription')}
            </p>
            <p className="mt-2 text-xs leading-5 text-[var(--color-text-tertiary)] max-w-3xl">
              <span className="material-symbols-outlined text-[12px] align-text-bottom mr-1">
                drag_handle
              </span>
              {t('settings.skills.dropHint')}
            </p>
          </div>

          <div className="grid grid-cols-2 gap-3 min-w-0 sm:grid-cols-3">
            <SummaryCard
              label={t('settings.skills.summary.totalSkills')}
              value={String(skills.length)}
              icon="auto_awesome"
            />
            <SummaryCard
              label={t('settings.skills.summary.sources')}
              value={String(
                SOURCE_ORDER.filter((source) => (grouped[source] ?? []).length > 0)
                  .length,
              )}
              icon="layers"
            />
            <SummaryCard
              label={t('settings.skills.summary.tokens')}
              value={t('settings.skills.tokenEstimateShort', { count: String(totalTokens) })}
              icon="notes"
              className="col-span-2 sm:col-span-1"
            />
          </div>
        </div>
      </section>

      <div className={`grid gap-4 ${visibleGroupCount >= 2 ? 'xl:grid-cols-2' : ''}`}>
        {SOURCE_ORDER.map((source) => {
          const group = grouped[source]
          if (!group?.length) return null

          const sourceLabel = t(`settings.skills.source.${source}`)
          const sourceTokenCount = group.reduce(
            (sum, skill) => sum + estimateTokens(skill.contentLength),
            0,
          )

          return (
            <section
              key={source}
              className="rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] overflow-hidden min-w-0"
            >
              <div className="flex items-start justify-between gap-3 px-4 py-3 border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
                <div className="min-w-0">
                  <div className="flex items-center gap-2 mb-1">
                    <span className={`inline-flex h-7 w-7 items-center justify-center rounded-full ${SOURCE_ACCENT_CLASSES[source]}`}>
                      <span className="material-symbols-outlined text-[14px]">
                        {SOURCE_ICONS[source]}
                      </span>
                    </span>
                    <h4 className="text-xs font-semibold text-[var(--color-text-primary)]">
                      {sourceLabel}
                    </h4>
                    <span className="text-xs text-[var(--color-text-tertiary)]">
                      {group.length}
                    </span>
                  </div>
                  <p className="text-xs leading-5 text-[var(--color-text-tertiary)]">
                    {t('settings.skills.groupHint', {
                      source: sourceLabel,
                      count: String(group.length),
                    })}
                  </p>
                </div>
                <div className="text-xs text-[var(--color-text-tertiary)] whitespace-nowrap">
                  {t('settings.skills.tokenEstimateShort', { count: String(sourceTokenCount) })}
                </div>
              </div>

              <div className="flex flex-col p-2">
                {group.map((skill) => (
                  <div
                    key={`${skill.source}-${skill.name}`}
                    role="button"
                    tabIndex={skill.hasDirectory ? 0 : -1}
                    onClick={() =>
                      skill.hasDirectory &&
                      fetchSkillDetail(skill.source, skill.name, currentWorkDir, 'skills')
                    }
                    onKeyDown={(e) => {
                      if (
                        skill.hasDirectory &&
                        (e.key === 'Enter' || e.key === ' ')
                      ) {
                        e.preventDefault()
                        fetchSkillDetail(
                          skill.source,
                          skill.name,
                          currentWorkDir,
                          'skills',
                        )
                      }
                    }}
                    aria-disabled={!skill.hasDirectory}
                    className={`group rounded-xl border border-transparent px-3 py-3 text-left transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-brand)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--color-surface)] ${
                      skill.hasDirectory
                        ? 'cursor-pointer hover:border-[var(--color-border-focus)] hover:bg-[var(--color-surface-hover)]'
                        : 'opacity-60 cursor-default'
                    }`}
                  >
                    <div className="flex items-start gap-3">
                      <span className="mt-0.5 material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]">
                        auto_awesome
                      </span>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 flex-wrap">
                          <span className="text-xs font-semibold text-[var(--color-text-primary)] break-all">
                            {skill.displayName || skill.name}
                          </span>
                          {skill.version && (
                            <span className="rounded-full bg-[var(--color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--color-text-tertiary)]">
                              v{skill.version}
                            </span>
                          )}
                          {skill.userInvocable && (
                            <span className="rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] font-medium text-[var(--color-text-tertiary)]">
                              {t('settings.skills.slashCommand')}
                            </span>
                          )}
                          {skill.always_apply && (
                            <span
                              title={t('settings.skills.alwaysApplyHint')}
                              className="rounded-full bg-[var(--color-success-container)] text-[var(--color-success)] px-2 py-0.5 text-[10px] font-medium"
                            >
                              {t('settings.skills.alwaysApplyBadge')}
                            </span>
                          )}
                        </div>
                        <p className="mt-1 text-xs leading-5 text-[var(--color-text-secondary)] break-words">
                          {skill.description}
                        </p>
                        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-[var(--color-text-tertiary)]">
                          <span>{sourceLabel}</span>
                          <span>{t('settings.skills.tokenEstimateShort', { count: String(estimateTokens(skill.contentLength)) })}</span>
                          <span>{skill.hasDirectory ? t('settings.skills.ready') : t('settings.skills.unavailable')}</span>
                        </div>
                      </div>
                      <div className="flex items-center gap-1 flex-shrink-0">
                        {source === 'user' && (
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation()
                              setPendingDelete(skill)
                            }}
                            title={t('common.delete')}
                            className="opacity-0 group-hover:opacity-100 transition-opacity inline-flex items-center justify-center w-7 h-7 rounded-md hover:bg-[var(--color-error-container)] text-[var(--color-text-tertiary)] hover:text-[var(--color-error)]"
                          >
                            <span className="material-symbols-outlined text-[14px]">
                              delete
                            </span>
                          </button>
                        )}
                        <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)] opacity-60 transition-transform group-hover:translate-x-0.5 group-hover:opacity-100">
                          chevron_right
                        </span>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </section>
          )
        })}
      </div>

      <CreateSkillDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreated={() => setCreateOpen(false)}
      />

      {pendingDelete && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4">
          <div className="w-full max-w-md rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 space-y-3 shadow-xl">
            <div>
              <p className="text-xs font-semibold text-[var(--color-text-primary)]">
                {t('settings.skills.deleteConfirmTitle')}
              </p>
              <p className="mt-1 text-xs text-[var(--color-text-secondary)] break-all">
                {t('settings.skills.deleteConfirmBody', { name: pendingDelete.name })}
              </p>
            </div>
            <div className="flex items-center justify-end gap-2">
              <button
                onClick={() => setPendingDelete(null)}
                className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]"
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={() => void handleConfirmDeleteSkill()}
                className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] bg-[var(--color-error)] text-white hover:opacity-90"
              >
                {t('common.delete')}
              </button>
            </div>
          </div>
        </div>
      )}

      <DropOverlay
        visible={isDragOver}
        installing={installing}
        title={t('settings.skills.dropOverlayTitle')}
        hint={t('settings.skills.dropOverlayHint')}
        installingLabel={t('settings.skills.installing')}
      />
      {conflictReports.length > 0 && (
        <ConflictDialog
          reports={conflictReports}
          onCancel={() => setConflictReports([])}
          onOverwrite={() => void handleConflictResolve('overwrite')}
          onKeepBoth={() => void handleConflictResolve('rename')}
          title={
            conflictReports.length > 1
              ? t('settings.skills.conflictTitlePlural')
              : t('settings.skills.conflictTitle')
          }
          body={t('settings.skills.conflictBody')}
          cancelLabel={t('common.cancel')}
          overwriteLabel={t('settings.skills.conflictOverwrite')}
          keepBothLabel={t('settings.skills.conflictKeepBoth')}
        />
      )}
    </div>
  )
}

function isPointInsideElement(el: HTMLElement, x: number, y: number) {
  const rect = el.getBoundingClientRect()
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1
  const px = x / dpr
  const py = y / dpr
  return px >= rect.left && px <= rect.right && py >= rect.top && py <= rect.bottom
}

function DropOverlay({
  visible,
  installing,
  title,
  hint,
  installingLabel,
}: {
  visible: boolean
  installing: boolean
  title: string
  hint: string
  installingLabel: string
}) {
  if (!visible && !installing) return null
  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center rounded-2xl border-2 border-dashed border-[var(--color-brand)] bg-[var(--color-brand)]/10 backdrop-blur-sm pointer-events-none">
      <div className="flex flex-col items-center gap-2 text-center px-6">
        {installing ? (
          <>
            <div className="animate-spin w-6 h-6 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
            <p className="text-xs font-semibold text-[var(--color-brand)]">{installingLabel}</p>
          </>
        ) : (
          <>
            <span className="material-symbols-outlined text-[32px] text-[var(--color-brand)]">
              upload_file
            </span>
            <p className="text-xs font-semibold text-[var(--color-brand)]">{title}</p>
            <p className="text-xs text-[var(--color-text-secondary)]">{hint}</p>
          </>
        )}
      </div>
    </div>
  )
}

function ConflictDialog({
  reports,
  onCancel,
  onOverwrite,
  onKeepBoth,
  title,
  body,
  cancelLabel,
  overwriteLabel,
  keepBothLabel,
}: {
  reports: SkillInstallReport[]
  onCancel: () => void
  onOverwrite: () => void
  onKeepBoth: () => void
  title: string
  body: string
  cancelLabel: string
  overwriteLabel: string
  keepBothLabel: string
}) {
  useEffect(() => {
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    document.addEventListener('keydown', handleEsc)
    return () => document.removeEventListener('keydown', handleEsc)
  }, [onCancel])

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4">
      <div className="w-full max-w-md rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 space-y-3 shadow-xl">
        <div>
          <p className="text-xs font-semibold text-[var(--color-text-primary)]">{title}</p>
          <p className="mt-1 text-xs text-[var(--color-text-secondary)]">{body}</p>
          <ul className="mt-2 list-disc list-inside text-xs text-[var(--color-text-primary)] space-y-0.5">
            {reports.map((r) => (
              <li key={`${r.source}-${r.name ?? ''}`} className="font-mono break-all">
                {r.name ?? r.source}
                {r.status === 'duplicate' ? (
                  <span className="ml-1 text-[10px] uppercase tracking-wide text-[var(--color-warning)]">
                    (in batch)
                  </span>
                ) : null}
              </li>
            ))}
          </ul>
        </div>
        <div className="flex items-center justify-end gap-2 flex-wrap">
          <button
            onClick={onCancel}
            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]"
          >
            {cancelLabel}
          </button>
          <button
            onClick={onKeepBoth}
            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] border border-[var(--color-brand)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-brand)]"
          >
            {keepBothLabel}
          </button>
          <button
            onClick={onOverwrite}
            className="inline-flex items-center gap-1.5 whitespace-nowrap h-7 px-2.5 text-xs rounded-[var(--radius-md)] bg-[var(--color-error)] text-white hover:opacity-90"
          >
            {overwriteLabel}
          </button>
        </div>
      </div>
    </div>
  )
}

function SummaryCard({
  label,
  value,
  icon,
  className = '',
}: {
  label: string
  value: string
  icon: string
  className?: string
}) {
  return (
    <div className={`rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3 min-w-0 ${className}`}>
      <div className="flex items-center gap-1.5 text-xs uppercase tracking-[0.12em] text-[var(--color-text-tertiary)] min-w-0">
        <span className="material-symbols-outlined text-[14px] flex-shrink-0">{icon}</span>
        <span className="truncate">{label}</span>
      </div>
      <div className="mt-2 text-xs font-semibold text-[var(--color-text-primary)] truncate">
        {value}
      </div>
    </div>
  )
}
