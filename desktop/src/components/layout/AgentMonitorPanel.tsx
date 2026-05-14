// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { useTranslation, type TranslationKey } from '../../i18n'
import {
  filterAgentSnapshots,
  groupAgentSnapshots,
  useAgentMonitorStore,
  useAgentSnapshots,
  useAgentSummary,
  type AgentMonitorFilterMode,
  type AgentMonitorGroupBy,
} from '../../stores/agentMonitorStore'
import { useTabStore } from '../../stores/tabStore'
import { useChatStore } from '../../stores/chatStore'
import { useUIStore } from '../../stores/uiStore'
import {
  AGENT_STATUS_META,
  type AgentSnapshot,
  type AgentStatus,
} from '../../utils/agentStatus'
import { resolveSessionTitle } from '../../utils/sessionTitle'

const FILTER_TABS: Array<{ mode: AgentMonitorFilterMode; labelKey: TranslationKey }> = [
  { mode: 'all', labelKey: 'agentMonitor.filter.all' },
  { mode: 'active', labelKey: 'agentMonitor.filter.active' },
  { mode: 'errors', labelKey: 'agentMonitor.filter.errors' },
]

const GROUP_OPTIONS: Array<{ id: AgentMonitorGroupBy; labelKey: TranslationKey }> = [
  { id: 'workspace', labelKey: 'agentMonitor.groupBy.workspace' },
  { id: 'status', labelKey: 'agentMonitor.groupBy.status' },
  { id: 'flat', labelKey: 'agentMonitor.groupBy.flat' },
]

function workspaceBasename(path: string): string {
  const trimmed = path.trim().replace(/[\\/]+$/, '')
  if (!trimmed) return ''
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

export function AgentMonitorPanel() {
  const t = useTranslation()
  const sidebarOpen = useUIStore((s) => s.sidebarOpen)
  const expanded = useAgentMonitorStore((s) => s.expanded)
  const filterMode = useAgentMonitorStore((s) => s.filterMode)
  const groupBy = useAgentMonitorStore((s) => s.groupBy)
  const toggleExpanded = useAgentMonitorStore((s) => s.toggleExpanded)
  const setFilterMode = useAgentMonitorStore((s) => s.setFilterMode)
  const setGroupBy = useAgentMonitorStore((s) => s.setGroupBy)
  const [groupMenuOpen, setGroupMenuOpen] = useState(false)

  const snapshots = useAgentSnapshots()
  const summary = useAgentSummary(snapshots)

  const filtered = useMemo(
    () => filterAgentSnapshots(snapshots, filterMode),
    [snapshots, filterMode],
  )
  const unknownLabel = t('agentMonitor.unknownWorkspace')
  const buckets = useMemo(
    () => groupAgentSnapshots(filtered, groupBy, unknownLabel),
    [filtered, groupBy, unknownLabel],
  )

  if (!sidebarOpen) {
    return <CollapsedDot summary={summary} title={t('agentMonitor.title')} />
  }

  return (
    <div className="px-3 pb-2">
      <div className="rounded-[12px] border border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
        <button
          type="button"
          onClick={toggleExpanded}
          aria-expanded={expanded}
          aria-label={expanded ? t('agentMonitor.collapse') : t('agentMonitor.expand')}
          className="flex w-full items-center gap-2 rounded-[12px] px-2.5 py-1.5 text-left text-xs transition-colors hover:bg-[var(--color-sidebar-item-hover)]"
        >
          <span
            className="material-symbols-outlined text-[15px] text-[var(--color-text-secondary)] transition-transform duration-150"
            style={{ transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)' }}
            aria-hidden="true"
          >
            chevron_right
          </span>
          <span className="material-symbols-outlined text-[15px] text-[var(--color-brand)]">
            hub
          </span>
          <span className="flex-1 truncate font-semibold tracking-[-0.01em] text-[var(--color-text-primary)]">
            {t('agentMonitor.title')}
          </span>
          <SummaryBadges summary={summary} />
        </button>

        {expanded && (
          <div className="border-t border-[var(--color-border)]/60 px-2 pb-2 pt-2">
            <div className="mb-2 flex items-center gap-1">
              <div className="flex flex-1 items-center gap-0.5 rounded-[10px] bg-[var(--color-surface-container)] p-0.5">
                {FILTER_TABS.map((tab) => {
                  const active = filterMode === tab.mode
                  return (
                    <button
                      key={tab.mode}
                      type="button"
                      onClick={() => setFilterMode(tab.mode)}
                      aria-pressed={active}
                      className={`flex-1 rounded-[8px] px-2 py-1 text-[11px] font-medium transition-colors ${
                        active
                          ? 'bg-[var(--color-surface)] text-[var(--color-text-primary)] shadow-sm'
                          : 'text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]'
                      }`}
                    >
                      {t(tab.labelKey)}
                    </button>
                  )
                })}
              </div>
              <div className="relative">
                <button
                  type="button"
                  onClick={() => setGroupMenuOpen((v) => !v)}
                  aria-haspopup="true"
                  aria-expanded={groupMenuOpen}
                  title={t('agentMonitor.groupBy.menuLabel')}
                  className="flex h-7 w-7 items-center justify-center rounded-[8px] text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-sidebar-item-hover)] hover:text-[var(--color-text-primary)]"
                >
                  <span className="material-symbols-outlined text-[14px]">tune</span>
                </button>
                {groupMenuOpen && (
                  <>
                    <div
                      className="fixed inset-0 z-30"
                      aria-hidden="true"
                      onClick={() => setGroupMenuOpen(false)}
                    />
                    <div className="absolute right-0 top-8 z-40 w-40 rounded-[10px] border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-[var(--shadow-dropdown)]">
                      <div className="px-2.5 pb-1 pt-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
                        {t('agentMonitor.groupBy.menuLabel')}
                      </div>
                      {GROUP_OPTIONS.map((opt) => {
                        const active = groupBy === opt.id
                        return (
                          <button
                            key={opt.id}
                            type="button"
                            onClick={() => {
                              setGroupBy(opt.id)
                              setGroupMenuOpen(false)
                            }}
                            className={`flex w-full items-center justify-between px-2.5 py-1.5 text-left text-[11px] transition-colors ${
                              active
                                ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                                : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                            }`}
                          >
                            <span>{t(opt.labelKey)}</span>
                            {active && (
                              <span className="material-symbols-outlined text-[13px] text-[var(--color-brand)]">
                                check
                              </span>
                            )}
                          </button>
                        )
                      })}
                    </div>
                  </>
                )}
              </div>
            </div>

            {filtered.length === 0 ? (
              <div className="rounded-[8px] px-2.5 py-3 text-center text-[11px] text-[var(--color-text-tertiary)]">
                {filterMode === 'active'
                  ? t('agentMonitor.emptyActive')
                  : filterMode === 'errors'
                  ? t('agentMonitor.emptyErrors')
                  : t('agentMonitor.empty')}
              </div>
            ) : (
              <div className="max-h-[260px] overflow-y-auto pr-0.5">
                {buckets.map((bucket) => (
                  <BucketSection
                    key={bucket.key}
                    label={bucket.label}
                    groupBy={groupBy}
                    snapshots={bucket.snapshots}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

function SummaryBadges({
  summary,
}: {
  summary: ReturnType<typeof useAgentSummary>
}) {
  return (
    <span className="flex flex-shrink-0 items-center gap-1.5 text-[10px] tabular-nums">
      {summary.error > 0 && (
        <Badge color="var(--color-error)" glyph="error" value={summary.error} />
      )}
      {summary.waiting > 0 && (
        <Badge color="var(--color-warning)" glyph="pan_tool" value={summary.waiting} pulse />
      )}
      {summary.waitingResource > 0 && (
        <Badge
          color="var(--color-warning)"
          glyph="hourglass_top"
          value={summary.waitingResource}
          pulse
        />
      )}
      {summary.active > 0 && (
        <Badge
          color="var(--color-success)"
          glyph="play_circle"
          value={summary.active}
          pulse
        />
      )}
      {summary.active === 0 &&
        summary.error === 0 &&
        summary.waiting === 0 &&
        summary.waitingResource === 0 && (
          <span className="text-[var(--color-text-tertiary)]">{summary.total}</span>
        )}
    </span>
  )
}

function Badge({
  color,
  glyph,
  value,
  pulse,
}: {
  color: string
  glyph: string
  value: number
  pulse?: boolean
}) {
  return (
    <span
      className="inline-flex items-center gap-0.5 rounded-full px-1 py-px"
      style={{
        backgroundColor: `color-mix(in srgb, ${color} 14%, transparent)`,
        color,
      }}
    >
      <span
        className={`material-symbols-outlined text-[11px] ${pulse ? 'animate-pulse' : ''}`}
        aria-hidden="true"
      >
        {glyph}
      </span>
      <span className="font-semibold">{value}</span>
    </span>
  )
}

function CollapsedDot({
  summary,
  title,
}: {
  summary: ReturnType<typeof useAgentSummary>
  title: string
}) {
  const active =
    summary.active + summary.waiting + summary.waitingResource + summary.error
  if (active === 0) return null
  const color =
    summary.error > 0
      ? 'var(--color-error)'
      : summary.waitingResource > 0 || summary.waiting > 0
      ? 'var(--color-warning)'
      : 'var(--color-success)'
  return (
    <div className="flex justify-center pb-1.5" title={title}>
      <span
        className="inline-flex h-5 min-w-[20px] items-center justify-center rounded-full px-1 text-[10px] font-bold tabular-nums text-white"
        style={{ backgroundColor: color }}
      >
        {active}
      </span>
    </div>
  )
}

function BucketSection({
  label,
  groupBy,
  snapshots,
}: {
  label: string
  groupBy: AgentMonitorGroupBy
  snapshots: AgentSnapshot[]
}) {
  const t = useTranslation()
  const showHeader = groupBy !== 'flat' && label.length > 0
  const resolvedLabel =
    groupBy === 'status'
      ? t(`agentMonitor.status.${label}` as TranslationKey)
      : label
  return (
    <div className="mb-1">
      {showHeader && (
        <div className="px-2 pb-0.5 pt-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
          {resolvedLabel} · {snapshots.length}
        </div>
      )}
      <div className="space-y-0.5">
        {snapshots.map((snap) => (
          <AgentRow key={snap.sessionId} snapshot={snap} />
        ))}
      </div>
    </div>
  )
}

function AgentRow({ snapshot }: { snapshot: AgentSnapshot }) {
  const t = useTranslation()
  const meta = AGENT_STATUS_META[snapshot.status]
  const statusLabel = t(meta.i18nKey as TranslationKey)
  const wsLabel = snapshot.workDir ? workspaceBasename(snapshot.workDir) : ''
  const title = resolveSessionTitle(snapshot.title, t('sidebar.untitled'))
  const tooltipParts: string[] = [`${title} · ${statusLabel}`]
  if (snapshot.workDir) tooltipParts.push(snapshot.workDir)
  if (snapshot.toolName && snapshot.status === 'tool') {
    tooltipParts.push(snapshot.toolName)
  }
  if (snapshot.status === 'waiting_resource' && snapshot.resourceWaitCount > 0) {
    tooltipParts.push(
      t('agentMonitor.waitingResourceCount', { count: snapshot.resourceWaitCount }),
    )
    if (snapshot.firstResourceWait) {
      const fw = snapshot.firstResourceWait
      const kindText =
        fw.kind === 'file'
          ? fw.target
          : fw.kind === 'shell'
          ? t('chat.resourceWait.shell')
          : t('chat.resourceWait.browser')
      const holder = fw.holderTitle || fw.holderSessionId
      if (holder) {
        tooltipParts.push(`${kindText} ← ${holder}`)
      } else {
        tooltipParts.push(kindText)
      }
    }
  }
  if (snapshot.queueLen > 0) {
    tooltipParts.push(t('agentMonitor.queuedBadge', { count: snapshot.queueLen }))
  }
  if (snapshot.codingMode) {
    tooltipParts.push(`${t('agentMonitor.codingMode')}: ${snapshot.codingMode}`)
  }
  if (snapshot.isAttached) {
    tooltipParts.push(t('agentMonitor.attached'))
  } else {
    tooltipParts.push(t('agentMonitor.clickToFocus'))
  }

  const handleClick = () => {
    useTabStore.getState().openTab(snapshot.sessionId, title)
    useChatStore.getState().connectToSession(snapshot.sessionId)
  }

  return (
    <button
      type="button"
      onClick={handleClick}
      title={tooltipParts.join('\n')}
      className={`group/row flex w-full items-center gap-2 rounded-[8px] px-2 py-1 text-left text-[11px] transition-colors ${
        snapshot.isAttached
          ? 'bg-[var(--color-sidebar-item-active)] text-[var(--color-text-primary)]'
          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-sidebar-item-hover)]'
      }`}
    >
      <StatusGlyph status={snapshot.status} />
      <span className="flex-1 truncate font-medium tracking-[-0.01em]">{title}</span>
      {wsLabel && (
        <span className="flex-shrink-0 truncate text-[10px] text-[var(--color-text-tertiary)] opacity-80">
          {wsLabel}
        </span>
      )}
      {snapshot.queueLen > 0 && (
        <span
          className="flex-shrink-0 inline-flex items-center gap-0.5 rounded-full px-1 text-[10px] tabular-nums"
          style={{
            backgroundColor: 'color-mix(in srgb, var(--color-secondary) 14%, transparent)',
            color: 'var(--color-secondary)',
          }}
        >
          <span className="material-symbols-outlined text-[10px]" aria-hidden="true">
            schedule
          </span>
          {snapshot.queueLen}
        </span>
      )}
    </button>
  )
}

function StatusGlyph({ status }: { status: AgentStatus }) {
  const meta = AGENT_STATUS_META[status]
  return (
    <span
      className={`material-symbols-outlined flex-shrink-0 text-[14px] ${
        meta.pulse ? 'animate-pulse' : ''
      }`}
      style={{ color: meta.colorVar }}
      aria-hidden="true"
    >
      {meta.glyph}
    </span>
  )
}
