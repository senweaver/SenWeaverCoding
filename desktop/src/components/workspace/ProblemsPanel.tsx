// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding

import { useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useLspStore } from '../../stores/lspStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import type { LspDiagnostic } from '../../types/lsp'

type Props = {
  workDir: string
  onJump: (relPath: string, position: { line: number; character: number }) => void
}

const SEVERITY_KEY: Record<number, 'error' | 'warning' | 'info' | 'hint'> = {
  1: 'error',
  2: 'warning',
  3: 'info',
  4: 'hint',
}

const SEVERITY_ICON: Record<string, string> = {
  error: 'error',
  warning: 'warning',
  info: 'info',
  hint: 'lightbulb',
}

const SEVERITY_COLOR: Record<string, string> = {
  error: 'text-[var(--color-error)]',
  warning: 'text-[var(--color-warning)]',
  info: 'text-[var(--color-text-secondary)]',
  hint: 'text-[var(--color-text-tertiary)]',
}

function uriToRel(uri: string, workDir: string): string | null {
  if (!uri || !uri.startsWith('file://')) return null
  let p = uri.slice('file://'.length)
  try {
    p = decodeURI(p)
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

type SeverityKey = 'error' | 'warning' | 'info' | 'hint'
const ALL_SEVERITIES: SeverityKey[] = ['error', 'warning', 'info', 'hint']
type ScopeMode = 'openTabs' | 'workspace'

const EMPTY_DIAGNOSTICS_BY_URI: Record<
  string,
  { serverId: string; version: number | null; diagnostics: LspDiagnostic[] }
> = Object.freeze({}) as Record<
  string,
  { serverId: string; version: number | null; diagnostics: LspDiagnostic[] }
>

export function ProblemsPanel({ workDir, onJump }: Props) {
  const t = useTranslation()
  const diagnosticsByUri = useLspStore((s) => {
    if (workDir && workDir.length > 0) {
      return s.diagnosticsByWorkspace[workDir] ?? EMPTY_DIAGNOSTICS_BY_URI
    }
    return s.diagnosticsByUri
  })
  const openTabs = useWorkspaceFilesStore((s) => s.openTabs)
  const [collapsed, setCollapsed] = useState(false)
  const [enabledSeverities, setEnabledSeverities] = useState<Set<SeverityKey>>(
    () => new Set<SeverityKey>(ALL_SEVERITIES),
  )
  const [scope, setScope] = useState<ScopeMode>('openTabs')

  const toggleSeverity = (sev: SeverityKey) => {
    setEnabledSeverities((prev) => {
      const next = new Set(prev)
      if (next.has(sev)) {
        if (next.size === 1) return prev
        next.delete(sev)
      } else {
        next.add(sev)
      }
      return next
    })
  }

  const grouped = useMemo(() => {
    const out: Array<{
      uri: string
      relPath: string
      diagnostics: LspDiagnostic[]
    }> = []
    const openSet = new Set(openTabs)
    for (const [uri, entry] of Object.entries(diagnosticsByUri)) {
      const list = entry?.diagnostics ?? []
      if (list.length === 0) continue
      const rel = uriToRel(uri, workDir)
      if (rel === null) continue
      if (scope === 'openTabs' && !openSet.has(rel)) continue
      const filtered = list.filter((diag) => {
        const sev = SEVERITY_KEY[diag.severity ?? 1] ?? 'error'
        return enabledSeverities.has(sev)
      })
      if (filtered.length === 0) continue
      out.push({ uri, relPath: rel, diagnostics: filtered })
    }
    out.sort((a, b) => a.relPath.localeCompare(b.relPath))
    return out
  }, [diagnosticsByUri, enabledSeverities, openTabs, scope, workDir])

  const totalCount = grouped.reduce((acc, g) => acc + g.diagnostics.length, 0)

  return (
    <div className="flex flex-shrink-0 flex-col border-t border-[var(--color-border)]">
      <button
        type="button"
        onClick={() => setCollapsed((c) => !c)}
        className="sticky top-0 z-[8] flex h-7 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
      >
        <span className="flex items-center gap-1">
          <span className="material-symbols-outlined text-[14px]">
            {collapsed ? 'chevron_right' : 'expand_more'}
          </span>
          {t('files.problems.title')}
        </span>
        <span className="text-[10px] tabular-nums text-[var(--color-text-tertiary)]/70">
          {totalCount > 0 ? totalCount : ''}
        </span>
      </button>
      {!collapsed && (
        <div className="flex flex-wrap items-center gap-1.5 border-b border-[var(--color-border)]/60 bg-[var(--color-surface-container-low)] px-2 py-1 text-[10px] text-[var(--color-text-tertiary)]">
          {ALL_SEVERITIES.map((sev) => {
            const enabled = enabledSeverities.has(sev)
            const label = t(`files.problems.severity.${sev}` as const)
            return (
              <button
                key={sev}
                type="button"
                onClick={() => toggleSeverity(sev)}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors ${
                  enabled
                    ? 'bg-[var(--color-surface)] text-[var(--color-text-secondary)]'
                    : 'opacity-40 hover:opacity-70'
                }`}
                title={label}
              >
                <span className={`material-symbols-outlined text-[11px] ${SEVERITY_COLOR[sev]}`}>
                  {SEVERITY_ICON[sev]}
                </span>
                <span className="uppercase">{sev}</span>
              </button>
            )
          })}
          <span className="mx-1 h-3 w-px bg-[var(--color-border)]" aria-hidden="true" />
          <button
            type="button"
            onClick={() => setScope('openTabs')}
            className={`rounded px-1.5 py-0.5 transition-colors ${
              scope === 'openTabs'
                ? 'bg-[var(--color-surface)] text-[var(--color-text-secondary)]'
                : 'opacity-60 hover:opacity-90'
            }`}
            title={t('files.problems.scope.open')}
          >
            {t('files.problems.scope.open')}
          </button>
          <button
            type="button"
            onClick={() => setScope('workspace')}
            className={`rounded px-1.5 py-0.5 transition-colors ${
              scope === 'workspace'
                ? 'bg-[var(--color-surface)] text-[var(--color-text-secondary)]'
                : 'opacity-60 hover:opacity-90'
            }`}
            title={t('files.problems.scope.workspace')}
          >
            {t('files.problems.scope.workspace')}
          </button>
        </div>
      )}
      {!collapsed && (
        <div className="max-h-[260px] overflow-y-auto">
          {grouped.length === 0 && (
            <div className="px-3 py-2 text-[11px] text-[var(--color-text-tertiary)] italic">
              {t('files.problems.empty')}
            </div>
          )}
          {grouped.map((group) => (
            <div key={group.uri} className="border-b border-[var(--color-border)]/60 last:border-b-0">
              <div className="flex items-center gap-1 bg-[var(--color-surface-container-low)] px-2 py-1 text-[11px] text-[var(--color-text-secondary)]">
                <span className="material-symbols-outlined text-[12px]">draft</span>
                <span className="truncate" title={group.relPath}>
                  {group.relPath}
                </span>
                <span className="ml-auto text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
                  {group.diagnostics.length}
                </span>
              </div>
              {group.diagnostics.map((diag, idx) => {
                const severity = SEVERITY_KEY[diag.severity ?? 1] ?? 'error'
                const lineLabel = `L${(diag.range?.start?.line ?? 0) + 1}`
                return (
                  <button
                    key={`${group.uri}-${idx}`}
                    type="button"
                    onClick={() => onJump(group.relPath, diag.range?.start ?? { line: 0, character: 0 })}
                    className="flex w-full items-start gap-1.5 px-2 py-1 text-left text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                    title={t('files.problems.openFile')}
                  >
                    <span
                      className={`material-symbols-outlined text-[12px] flex-shrink-0 ${SEVERITY_COLOR[severity]}`}
                    >
                      {SEVERITY_ICON[severity]}
                    </span>
                    <span className="min-w-0 flex-1 truncate">{diag.message}</span>
                    <span className="ml-1 flex-shrink-0 text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
                      {lineLabel}
                    </span>
                  </button>
                )
              })}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
