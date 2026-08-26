// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { skillsApi } from '../../api/skills'
import { mcpApi } from '../../api/mcp'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useMcpStore } from '../../stores/mcpStore'
import { useSkillStore } from '../../stores/skillStore'
import type { McpServerRecord } from '../../types/mcp'
import type { SkillMeta } from '../../types/skill'

export type LocalSlashCommandName = 'mcp' | 'skills'

type Props = {
  command: LocalSlashCommandName
  cwd?: string
  onClose: () => void
}

function toneForStatus(status: McpServerRecord['status']) {
  switch (status) {
    case 'connected':
      return 'bg-emerald-500/10 text-emerald-600 border-emerald-500/20'
    case 'needs-auth':
      return 'bg-amber-500/10 text-amber-600 border-amber-500/20'
    case 'failed':
      return 'bg-rose-500/10 text-rose-600 border-rose-500/20'
    case 'disabled':
      return 'bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)] border-[var(--color-border)]'
    default:
      return ''
  }
}

function scopeLabel(scope: string, t: ReturnType<typeof useTranslation>) {
  switch (scope) {
    case 'user':
      return t('settings.mcp.scope.user')
    case 'local':
      return t('settings.mcp.scope.local')
    case 'project':
      return t('settings.mcp.scope.project')
    default:
      return scope
  }
}

function projectBadge(path?: string, t?: ReturnType<typeof useTranslation>) {
  if (!path || !t) return null
  const label = path.replace(/\/$/, '').split('/').pop() || path
  return t('slash.mcp.projectBadge', { name: label })
}

function PanelShell({
  title,
  subtitle,
  children,
  onClose,
}: {
  title: string
  subtitle: string
  children: React.ReactNode
  onClose: () => void
}) {
  return (
    <div
      id="local-slash-command-panel"
      className="absolute bottom-full left-0 right-0 z-50 mb-3 overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]"
    >
      <div className="flex items-start justify-between gap-4 border-b border-[var(--color-border)] px-5 py-4">
        <div>
          <h3 className="text-lg font-semibold text-[var(--color-text-primary)]">{title}</h3>
          <p className="mt-1 text-sm text-[var(--color-text-tertiary)]">{subtitle}</p>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="flex h-9 w-9 items-center justify-center rounded-full text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[18px]">close</span>
        </button>
      </div>
      <div className="max-h-[420px] overflow-y-auto px-5 py-4">{children}</div>
    </div>
  )
}

function LoadingState({ label }: { label: string }) {
  return (
    <div className="flex items-center justify-center py-12 text-sm text-[var(--color-text-tertiary)]">
      <div className="mr-3 h-5 w-5 animate-spin rounded-full border-2 border-[var(--color-brand)] border-t-transparent" />
      {label}
    </div>
  )
}

function EmptyState({ title, body }: { title: string; body: string }) {
  return (
    <div className="rounded-2xl border border-dashed border-[var(--color-border)] bg-[var(--color-surface)] px-5 py-10 text-center">
      <div className="text-sm font-semibold text-[var(--color-text-primary)]">{title}</div>
      <div className="mt-2 text-xs leading-6 text-[var(--color-text-tertiary)]">{body}</div>
    </div>
  )
}

function ErrorState({ message }: { message: string }) {
  return (
    <div className="rounded-2xl border border-[var(--color-error)]/20 bg-[var(--color-error)]/8 px-5 py-4 text-sm text-[var(--color-error)]">
      {message}
    </div>
  )
}

function McpPanel({ cwd, onClose }: { cwd?: string; onClose: () => void }) {
  const t = useTranslation()
  const openSettingsOverlay = useUIStore((s) => s.openSettingsOverlay)
  const selectServer = useMcpStore((s) => s.selectServer)
  const [servers, setServers] = useState<McpServerRecord[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    mcpApi.list(cwd)
      .then(async (response) => {
        if (cancelled) return
        const visibleServers = response.servers.filter((server) => server.scope === 'user' || server.scope === 'local' || server.scope === 'project')
        setServers(visibleServers)

        const statusResults = await Promise.allSettled(
          visibleServers.map((server) => mcpApi.status(server.name, cwd)),
        )
        if (cancelled) return

        const liveServers = new Map<string, McpServerRecord>()
        for (const result of statusResults) {
          if (result.status === 'fulfilled') {
            liveServers.set(result.value.server.name, result.value.server)
          }
        }
        if (liveServers.size > 0) {
          setServers((current) =>
            current?.map((server) => liveServers.get(server.name) ?? server) ?? current,
          )
        }
      })
      .catch((err) => {
        if (cancelled) return
        setError(err instanceof Error ? err.message : String(err))
      })
    return () => {
      cancelled = true
    }
  }, [cwd])

  const grouped = useMemo(() => {
    const groups = new Map<string, McpServerRecord[]>()
    for (const server of servers ?? []) {
      const key = server.scope
      const existing = groups.get(key) ?? []
      existing.push(server)
      groups.set(key, existing)
    }
    return groups
  }, [servers])

  return (
    <PanelShell
      title={t('slash.mcp.title')}
      subtitle={cwd ? t('slash.mcp.subtitleWithProject', { path: cwd }) : t('slash.mcp.subtitle')}
      onClose={onClose}
    >
      {error ? (
        <ErrorState message={error} />
      ) : servers === null ? (
        <LoadingState label={t('common.loading')} />
      ) : servers.length === 0 ? (
        <EmptyState title={t('slash.mcp.emptyTitle')} body={t('slash.mcp.emptyBody')} />
      ) : (
        <div className="space-y-5">
          {['user', 'local', 'project'].filter((scope) => grouped.has(scope)).map((scope) => (
            <section key={scope}>
              <div className="mb-2 flex items-center justify-between">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">{scopeLabel(scope, t)}</div>
                <div className="text-xs text-[var(--color-text-tertiary)]">{grouped.get(scope)?.length ?? 0}</div>
              </div>
              <div className="overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]">
                {grouped.get(scope)?.map((server) => (
                  <button
                    type="button"
                    key={`${server.scope}:${server.projectPath ?? 'global'}:${server.name}`}
                    onClick={() => {
                      selectServer(server)
                      useUIStore.getState().setPendingCustomSubTab('mcps')
                      openSettingsOverlay('custom')
                      onClose()
                    }}
                    className="block w-full border-t border-[var(--color-border)] px-4 py-4 text-left first:border-t-0 hover:bg-[var(--color-surface-hover)]"
                  >
                    <div className="flex items-center gap-3">
                      <div className="text-sm font-semibold text-[var(--color-text-primary)]">{server.name}</div>
                      <span className={`inline-flex items-center rounded-full border px-2 py-1 text-[11px] font-semibold ${toneForStatus(server.status)}`}>
                        {server.statusLabel}
                      </span>
                    </div>
                    <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-[var(--color-text-tertiary)]">
                      <span className="rounded-full bg-[var(--color-surface-hover)] px-2 py-1">{server.transport}</span>
                      {server.projectPath && (
                        <span className="rounded-full bg-[var(--color-surface-hover)] px-2 py-1" title={server.projectPath}>
                          {projectBadge(server.projectPath, t)}
                        </span>
                      )}
                      <span className="truncate">{server.summary}</span>
                    </div>
                  </button>
                ))}
              </div>
            </section>
          ))}
        </div>
      )}
    </PanelShell>
  )
}

function SkillsPanel({ cwd, onClose }: { cwd?: string; onClose: () => void }) {
  const t = useTranslation()
  const openSettingsOverlay = useUIStore((s) => s.openSettingsOverlay)
  const fetchSkillDetail = useSkillStore((s) => s.fetchSkillDetail)
  const [skills, setSkills] = useState<SkillMeta[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    skillsApi.list(cwd)
      .then((response) => {
        if (cancelled) return
        setSkills(response.skills.filter((skill) => skill.userInvocable))
      })
      .catch((err) => {
        if (cancelled) return
        setError(err instanceof Error ? err.message : String(err))
      })
    return () => {
      cancelled = true
    }
  }, [cwd])

  return (
    <PanelShell
      title={t('slash.skills.title')}
      subtitle={cwd ? t('slash.skills.subtitleWithProject', { path: cwd }) : t('slash.skills.subtitle')}
      onClose={onClose}
    >
      {error ? (
        <ErrorState message={error} />
      ) : skills === null ? (
        <LoadingState label={t('common.loading')} />
      ) : skills.length === 0 ? (
        <EmptyState title={t('slash.skills.emptyTitle')} body={t('slash.skills.emptyBody')} />
      ) : (
        <div className="overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)]">
          {skills.map((skill) => (
            <button
              type="button"
              key={`${skill.source}:${skill.name}`}
              onClick={async () => {
                await fetchSkillDetail(skill.source, skill.name, cwd, 'skills')
                useUIStore.getState().setPendingCustomSubTab('skills')
                openSettingsOverlay('custom')
                onClose()
              }}
              className="block w-full border-t border-[var(--color-border)] px-4 py-4 text-left first:border-t-0 hover:bg-[var(--color-surface-hover)]"
            >
              <div className="flex items-center gap-3">
                <div className="text-sm font-semibold text-[var(--color-text-primary)]">/{skill.name}</div>
                <span className="rounded-full bg-[var(--color-surface-hover)] px-2 py-1 text-[11px] text-[var(--color-text-secondary)]">
                  {skill.source}
                </span>
              </div>
              <div className="mt-2 text-xs leading-6 text-[var(--color-text-tertiary)]">{skill.description}</div>
            </button>
          ))}
        </div>
      )}
    </PanelShell>
  )
}

export function LocalSlashCommandPanel({ command, cwd, onClose }: Props) {
  if (command === 'mcp') return <McpPanel cwd={cwd} onClose={onClose} />
  return <SkillsPanel cwd={cwd} onClose={onClose} />
}
