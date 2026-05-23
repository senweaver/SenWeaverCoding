import { useEffect, useMemo } from 'react'
import { useTranslation } from '../i18n'
import type { TranslationKey } from '../i18n'
import { Button } from '../components/shared/Button'
import { useUsageStore } from '../stores/usageStore'
import { useSessionStore } from '../stores/sessionStore'
import { useRuntimeStore } from '../stores/runtimeStore'
import { resolveSessionTitle } from '../utils/sessionTitle'
import type { SessionListItem } from '../types/session'
import type { CodingModeId } from '../types/codingMode'
import type {
  UsageCodingModeStats,
  UsageLifetimeStats,
  UsageProviderStats,
  UsageSessionStats,
} from '../types/usage'

const CODING_MODE_BACKEND_TO_ID: Record<string, CodingModeId> = {
  agent: 'agent',
  plan: 'plan',
  ask: 'ask',
  debug: 'debug',
  harness: 'harness',
}

const CODING_MODE_ORDER: CodingModeId[] = [
  'agent',
  'plan',
  'ask',
  'debug',
  'harness',
]

const CODING_MODE_ICON: Record<CodingModeId, string> = {
  vibe: '∞',
  spec: '📋',
  plan: '📄',
  ask: '💬',
  tdd: '🔬',
  debug: '🐛',
  agent: '🤖',
  architect: '🏛',
  pair: '🤝',
  context: '🧠',
  mvai: '⚡',
  harness: '🛡',
  curator: '📚',
}

function formatUsd(value: number): string {
  if (!Number.isFinite(value)) return '$0.00'
  if (value >= 1) return `$${value.toFixed(2)}`
  if (value > 0) return `$${value.toFixed(4)}`
  return '$0.00'
}

function formatNumber(value: number): string {
  return Math.round(value).toLocaleString()
}

function formatCompact(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0'
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`
  return Math.round(value).toLocaleString()
}

function formatTimestamp(value: string | null): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString()
  } catch {
    return value
  }
}

function formatUptime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '0s'
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const secs = Math.floor(seconds % 60)
  const parts: string[] = []
  if (days > 0) parts.push(`${days}d`)
  if (hours > 0 || days > 0) parts.push(`${hours}h`)
  if (minutes > 0 || hours > 0 || days > 0) parts.push(`${minutes}m`)
  parts.push(`${secs}s`)
  return parts.join(' ')
}

function workspaceBasename(p: string | null | undefined): string {
  if (!p) return ''
  const trimmed = p.trim().replace(/[\\/]+$/, '')
  if (!trimmed) return ''
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

type SessionRow = {
  stats: UsageSessionStats
  session: SessionListItem | undefined
  deleted: boolean
}

type WorkspaceRow = {
  workspaceKey: string
  workspaceLabel: string
  sessionCount: number
  totalTokens: number
  inputTokens: number
  outputTokens: number
  costUsd: number
  requestCount: number
  lastUsed: string | null
}

function sortByLastUsedDesc(a: UsageSessionStats, b: UsageSessionStats): number {
  const aT = a.lastUsed ? Date.parse(a.lastUsed) : 0
  const bT = b.lastUsed ? Date.parse(b.lastUsed) : 0
  return bT - aT
}

export function UsageSettings() {
  const t = useTranslation()
  const summary = useUsageStore((s) => s.summary)
  const isLoading = useUsageStore((s) => s.isLoading)
  const error = useUsageStore((s) => s.error)
  const fetchUsage = useUsageStore((s) => s.fetch)
  const sessions = useSessionStore((s) => s.sessions)
  const runtimeSnapshot = useRuntimeStore((s) => s.snapshot)
  const runtimeError = useRuntimeStore((s) => s.error)
  const runtimeLoading = useRuntimeStore((s) => s.isLoading)
  const fetchRuntime = useRuntimeStore((s) => s.fetch)

  useEffect(() => {
    void fetchUsage()
    void fetchRuntime()
  }, [fetchUsage, fetchRuntime])

  const handleRefresh = () => {
    void fetchUsage()
    void fetchRuntime()
  }

  const lifetimeRows = useMemo<UsageLifetimeStats[]>(() => {
    if (!summary) return []
    return Object.values(summary.byModelLifetime).sort(
      (a, b) => b.totalTokens - a.totalTokens || b.costUsd - a.costUsd,
    )
  }, [summary])

  const providerRows = useMemo<UsageProviderStats[]>(() => {
    if (!summary) return []
    return Object.values(summary.byProvider).sort(
      (a, b) => b.totalTokens - a.totalTokens || b.costUsd - a.costUsd,
    )
  }, [summary])

  const codingModeRows = useMemo<Array<{ id: CodingModeId; stats: UsageCodingModeStats | null }>>(() => {
    const byId: Record<string, UsageCodingModeStats | null> = {}
    if (summary) {
      for (const [key, raw] of Object.entries(summary.byCodingMode)) {
        const id = CODING_MODE_BACKEND_TO_ID[key.toLowerCase()] ?? null
        if (id) byId[id] = raw
      }
    }
    return CODING_MODE_ORDER.map((id) => ({ id, stats: byId[id] ?? null }))
  }, [summary])

  const codingModeTotals = useMemo(() => {
    if (!summary) {
      return { totalTokens: 0, requestCount: 0, costUsd: 0, activeModes: 0 }
    }
    const values = Object.entries(summary.byCodingMode)
      .filter(([key]) => CODING_MODE_BACKEND_TO_ID[key.toLowerCase()] != null)
      .map(([, raw]) => raw)
    return {
      totalTokens: values.reduce((acc, r) => acc + r.totalTokens, 0),
      requestCount: values.reduce((acc, r) => acc + r.requestCount, 0),
      costUsd: values.reduce((acc, r) => acc + r.costUsd, 0),
      activeModes: values.filter((r) => r.requestCount > 0).length,
    }
  }, [summary])

  const sessionRows = useMemo<SessionRow[]>(() => {
    if (!summary) return []
    const byId = new Map<string, SessionListItem>()
    for (const s of sessions) byId.set(s.id, s)
    return Object.values(summary.bySession)
      .sort(sortByLastUsedDesc)
      .map((stats) => {
        const session = byId.get(stats.sessionId)
        return { stats, session, deleted: !session }
      })
  }, [summary, sessions])

  const workspaceRows = useMemo<WorkspaceRow[]>(() => {
    if (!summary) return []
    const byId = new Map<string, SessionListItem>()
    for (const s of sessions) byId.set(s.id, s)
    const buckets = new Map<string, WorkspaceRow>()
    for (const stats of Object.values(summary.bySession)) {
      const session = byId.get(stats.sessionId)
      const dir = session?.workDir ?? ''
      const label = workspaceBasename(dir) || (dir || t('settings.usage.workspaceUnknown'))
      const key = dir || `__unknown__:${label}`
      let row = buckets.get(key)
      if (!row) {
        row = {
          workspaceKey: key,
          workspaceLabel: label,
          sessionCount: 0,
          totalTokens: 0,
          inputTokens: 0,
          outputTokens: 0,
          costUsd: 0,
          requestCount: 0,
          lastUsed: null,
        }
        buckets.set(key, row)
      }
      row.sessionCount += 1
      row.totalTokens += stats.totalTokens
      row.inputTokens += stats.inputTokens
      row.outputTokens += stats.outputTokens
      row.costUsd += stats.costUsd
      row.requestCount += stats.requestCount
      const ts = stats.lastUsed ? Date.parse(stats.lastUsed) : 0
      const cur = row.lastUsed ? Date.parse(row.lastUsed) : 0
      if (ts > cur) row.lastUsed = stats.lastUsed
    }
    return Array.from(buckets.values()).sort(
      (a, b) => b.totalTokens - a.totalTokens || b.costUsd - a.costUsd,
    )
  }, [summary, sessions, t])

  const lifetimeTotalTokens = useMemo(() => {
    if (!summary) return 0
    return Object.values(summary.byModelLifetime).reduce(
      (acc, row) => acc + (row.totalTokens || 0),
      0,
    )
  }, [summary])

  const lifetimeRequestCount = useMemo(() => {
    if (!summary) return 0
    return Object.values(summary.byModelLifetime).reduce(
      (acc, row) => acc + (row.requestCount || 0),
      0,
    )
  }, [summary])

  const avgTokensPerRequest = useMemo(() => {
    if (lifetimeRequestCount <= 0) return 0
    return lifetimeTotalTokens / lifetimeRequestCount
  }, [lifetimeTotalTokens, lifetimeRequestCount])

  const activeSessionCount = sessions.length

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.title')}
          </h2>
          <p className="text-xs text-[var(--color-text-secondary)] mt-1">
            {t('settings.usage.description')}
          </p>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={handleRefresh}
          disabled={isLoading || runtimeLoading}
        >
          {isLoading || runtimeLoading
            ? t('common.loading')
            : t('common.refresh')}
        </Button>
      </div>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}
      {runtimeError && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {runtimeError}
        </div>
      )}

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
        <KpiCard
          label={t('settings.usage.kpiTotalTokens')}
          value={formatNumber(lifetimeTotalTokens)}
          hint={t('settings.usage.kpiTotalTokensHint')}
          emphasis
        />
        <KpiCard
          label={t('settings.usage.kpiRequests')}
          value={formatNumber(lifetimeRequestCount)}
          hint={t('settings.usage.kpiRequestsHint')}
        />
        <KpiCard
          label={t('settings.usage.kpiDaily')}
          value={formatUsd(summary?.dailyCostUsd ?? 0)}
          hint={t('settings.usage.kpiDailyHint')}
        />
        <KpiCard
          label={t('settings.usage.kpiMonthly')}
          value={formatUsd(summary?.monthlyCostUsd ?? 0)}
          hint={t('settings.usage.kpiMonthlyHint')}
        />
        <KpiCard
          label={t('settings.usage.kpiTokenRate')}
          value={`${formatCompact(summary?.tokenRatePerMin ?? 0)}/min`}
          hint={t('settings.usage.kpiTokenRateHint')}
        />
        <KpiCard
          label={t('settings.usage.kpiAvgTokens')}
          value={formatNumber(avgTokensPerRequest)}
          hint={t('settings.usage.kpiAvgTokensHint')}
        />
        <KpiCard
          label={t('settings.usage.kpi24h')}
          value={formatNumber(summary?.last24hTokens ?? 0)}
          hint={
            summary
              ? `${formatNumber(summary.last24hRequests)} · ${formatUsd(summary.last24hCostUsd)}`
              : t('settings.usage.kpi24hHint')
          }
        />
        <KpiCard
          label={t('settings.usage.kpi7d')}
          value={formatNumber(summary?.last7dTokens ?? 0)}
          hint={
            summary
              ? `${formatNumber(summary.last7dRequests)} · ${formatUsd(summary.last7dCostUsd)}`
              : t('settings.usage.kpi7dHint')
          }
        />
      </div>

      <SystemInfoCard
        snapshot={runtimeSnapshot}
        activeSessions={activeSessionCount}
        t={t}
      />

      <CodingModeSection
        rows={codingModeRows}
        totals={codingModeTotals}
        t={t}
      />

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
        <header className="flex items-baseline justify-between">
          <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.providerSection')}
          </h3>
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.usage.providerHint')}
          </span>
        </header>
        {providerRows.length === 0 ? (
          <p className="text-xs text-[var(--color-text-secondary)] py-6 text-center">
            {t('settings.usage.empty')}
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead className="text-[var(--color-text-tertiary)]">
                <tr className="border-b border-[var(--color-border)]/40">
                  <Th>{t('settings.usage.colProvider')}</Th>
                  <Th align="right">{t('settings.usage.colModelCount')}</Th>
                  <Th align="right">{t('settings.usage.colRequests')}</Th>
                  <Th align="right">{t('settings.usage.colInputTokens')}</Th>
                  <Th align="right">{t('settings.usage.colOutputTokens')}</Th>
                  <Th align="right">{t('settings.usage.colTotalTokens')}</Th>
                  <Th align="right">{t('settings.usage.colCost')}</Th>
                  <Th>{t('settings.usage.colLastUsed')}</Th>
                </tr>
              </thead>
              <tbody>
                {providerRows.map((row) => (
                  <tr
                    key={row.provider}
                    className="border-b border-[var(--color-border)]/20 last:border-0"
                  >
                    <td className="py-2 font-mono text-[var(--color-text-primary)]">
                      {row.provider}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.modelCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.requestCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.inputTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.outputTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums font-semibold text-[var(--color-text-primary)]">
                      {formatNumber(row.totalTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums text-[var(--color-text-secondary)]">
                      {formatUsd(row.costUsd)}
                    </td>
                    <td className="py-2 text-[var(--color-text-secondary)]">
                      {formatTimestamp(row.lastUsed)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
        <header className="flex items-baseline justify-between">
          <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.workspaceSection')}
          </h3>
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.usage.workspaceHint')}
          </span>
        </header>
        {workspaceRows.length === 0 ? (
          <p className="text-xs text-[var(--color-text-secondary)] py-6 text-center">
            {t('settings.usage.empty')}
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead className="text-[var(--color-text-tertiary)]">
                <tr className="border-b border-[var(--color-border)]/40">
                  <Th>{t('settings.usage.colWorkspace')}</Th>
                  <Th align="right">{t('settings.usage.colSessionCount')}</Th>
                  <Th align="right">{t('settings.usage.colRequests')}</Th>
                  <Th align="right">{t('settings.usage.colTotalTokens')}</Th>
                  <Th align="right">{t('settings.usage.colCost')}</Th>
                  <Th>{t('settings.usage.colLastUsed')}</Th>
                </tr>
              </thead>
              <tbody>
                {workspaceRows.map((row) => (
                  <tr
                    key={row.workspaceKey}
                    className="border-b border-[var(--color-border)]/20 last:border-0"
                  >
                    <td className="py-2 max-w-[260px]">
                      <span
                        className="truncate inline-block max-w-full text-[var(--color-text-primary)]"
                        title={row.workspaceLabel}
                      >
                        {row.workspaceLabel}
                      </span>
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.sessionCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.requestCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums font-semibold text-[var(--color-text-primary)]">
                      {formatNumber(row.totalTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums text-[var(--color-text-secondary)]">
                      {formatUsd(row.costUsd)}
                    </td>
                    <td className="py-2 text-[var(--color-text-secondary)]">
                      {formatTimestamp(row.lastUsed)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
        <header className="flex items-baseline justify-between">
          <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.lifetimeSection')}
          </h3>
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.usage.lifetimeHint')}
          </span>
        </header>
        {lifetimeRows.length === 0 ? (
          <p className="text-xs text-[var(--color-text-secondary)] py-6 text-center">
            {t('settings.usage.empty')}
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead className="text-[var(--color-text-tertiary)]">
                <tr className="border-b border-[var(--color-border)]/40">
                  <Th>{t('settings.usage.colModel')}</Th>
                  <Th align="right">{t('settings.usage.colRequests')}</Th>
                  <Th align="right">{t('settings.usage.colInputTokens')}</Th>
                  <Th align="right">{t('settings.usage.colOutputTokens')}</Th>
                  <Th align="right">{t('settings.usage.colTotalTokens')}</Th>
                  <Th align="right">{t('settings.usage.colCost')}</Th>
                  <Th>{t('settings.usage.colFirstUsed')}</Th>
                  <Th>{t('settings.usage.colLastUsed')}</Th>
                </tr>
              </thead>
              <tbody>
                {lifetimeRows.map((row) => (
                  <tr
                    key={row.model}
                    className="border-b border-[var(--color-border)]/20 last:border-0"
                  >
                    <td className="py-2 font-mono text-[var(--color-text-primary)]">
                      {row.model}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.requestCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.inputTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(row.outputTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums font-semibold text-[var(--color-text-primary)]">
                      {formatNumber(row.totalTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums text-[var(--color-text-secondary)]">
                      {formatUsd(row.costUsd)}
                    </td>
                    <td className="py-2 text-[var(--color-text-secondary)]">
                      {formatTimestamp(row.firstUsed)}
                    </td>
                    <td className="py-2 text-[var(--color-text-secondary)]">
                      {formatTimestamp(row.lastUsed)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
        <header className="flex items-baseline justify-between">
          <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.sessionSection')}
          </h3>
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.usage.sessionHint')}
          </span>
        </header>
        {sessionRows.length === 0 ? (
          <p className="text-xs text-[var(--color-text-secondary)] py-6 text-center">
            {t('settings.usage.empty')}
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead className="text-[var(--color-text-tertiary)]">
                <tr className="border-b border-[var(--color-border)]/40">
                  <Th>{t('settings.usage.colSession')}</Th>
                  <Th>{t('settings.usage.colWorkspace')}</Th>
                  <Th align="right">{t('settings.usage.colRequests')}</Th>
                  <Th align="right">{t('settings.usage.colInputTokens')}</Th>
                  <Th align="right">{t('settings.usage.colOutputTokens')}</Th>
                  <Th align="right">{t('settings.usage.colTotalTokens')}</Th>
                  <Th align="right">{t('settings.usage.colCost')}</Th>
                  <Th>{t('settings.usage.colLastUsed')}</Th>
                  <Th>{t('settings.usage.colFirstUsed')}</Th>
                </tr>
              </thead>
              <tbody>
                {sessionRows.map((row) => {
                  const { stats, session, deleted } = row
                  const title = session
                    ? resolveSessionTitle(session.title, t('sidebar.untitled'))
                    : t('settings.usage.deletedSession')
                  const idSuffix = stats.sessionId.slice(-8)
                  const workspace = session ? workspaceBasename(session.workDir) : ''
                  return (
                    <tr
                      key={stats.sessionId}
                      className="border-b border-[var(--color-border)]/20 last:border-0"
                    >
                      <td className="py-2 max-w-[260px]">
                        <div className="flex flex-col gap-0.5">
                          <span
                            className={`truncate ${
                              deleted
                                ? 'text-[var(--color-text-tertiary)] italic'
                                : 'text-[var(--color-text-primary)]'
                            }`}
                            title={title}
                          >
                            {title}
                          </span>
                          <span className="font-mono text-[10px] text-[var(--color-text-tertiary)]">
                            …{idSuffix}
                          </span>
                        </div>
                      </td>
                      <td className="py-2 text-[var(--color-text-secondary)] max-w-[180px]">
                        {workspace ? (
                          <span className="truncate inline-block max-w-full" title={workspace}>
                            {workspace}
                          </span>
                        ) : (
                          <span className="text-[var(--color-text-tertiary)]">—</span>
                        )}
                      </td>
                      <td className="py-2 text-right tabular-nums">
                        {formatNumber(stats.requestCount)}
                      </td>
                      <td className="py-2 text-right tabular-nums">
                        {formatNumber(stats.inputTokens)}
                      </td>
                      <td className="py-2 text-right tabular-nums">
                        {formatNumber(stats.outputTokens)}
                      </td>
                      <td className="py-2 text-right tabular-nums font-semibold text-[var(--color-text-primary)]">
                        {formatNumber(stats.totalTokens)}
                      </td>
                      <td className="py-2 text-right tabular-nums text-[var(--color-text-secondary)]">
                        {formatUsd(stats.costUsd)}
                      </td>
                      <td className="py-2 text-[var(--color-text-secondary)]">
                        {formatTimestamp(stats.lastUsed)}
                      </td>
                      <td className="py-2 text-[var(--color-text-secondary)]">
                        {formatTimestamp(stats.firstUsed)}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <BackgroundTasksSection snapshot={runtimeSnapshot} t={t} />
    </div>
  )
}

function SystemInfoCard({
  snapshot,
  activeSessions,
  t,
}: {
  snapshot: ReturnType<typeof useRuntimeStore.getState>['snapshot']
  activeSessions: number
  t: (key: TranslationKey) => string
}) {
  if (!snapshot) {
    return (
      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3">
        <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.usage.systemSection')}
        </h3>
        <p className="text-xs text-[var(--color-text-secondary)] py-4 text-center">
          {t('settings.usage.systemLoading')}
        </p>
      </section>
    )
  }

  const items: Array<{ label: string; value: string; mono?: boolean }> = [
    {
      label: t('settings.usage.systemVersion'),
      value: `${snapshot.version}${snapshot.buildProfile ? ` (${snapshot.buildProfile})` : ''}`,
    },
    {
      label: t('settings.usage.systemPlatform'),
      value: `${snapshot.platform}/${snapshot.arch}`,
    },
    {
      label: t('settings.usage.systemPid'),
      value: snapshot.pid > 0 ? String(snapshot.pid) : '—',
      mono: true,
    },
    {
      label: t('settings.usage.systemCpu'),
      value: snapshot.cpuCount > 0 ? String(snapshot.cpuCount) : '—',
    },
    {
      label: t('settings.usage.systemUptime'),
      value: formatUptime(snapshot.uptimeSecs),
    },
    {
      label: t('settings.usage.systemStartedAt'),
      value: formatTimestamp(snapshot.startedAt),
    },
    {
      label: t('settings.usage.systemGateway'),
      value: snapshot.gateway.url || `${snapshot.gateway.host}:${snapshot.gateway.port}`,
      mono: true,
    },
    {
      label: t('settings.usage.systemPathPrefix'),
      value: snapshot.gateway.pathPrefix || '/',
      mono: true,
    },
    {
      label: t('settings.usage.systemWorkspace'),
      value: snapshot.workspaceDir || '—',
      mono: true,
    },
    {
      label: t('settings.usage.systemDefaultProvider'),
      value: snapshot.defaultProvider || '—',
    },
    {
      label: t('settings.usage.systemDefaultModel'),
      value: snapshot.defaultModel || '—',
      mono: true,
    },
    {
      label: t('settings.usage.systemActiveSessions'),
      value: String(activeSessions),
    },
    {
      label: t('settings.usage.systemLiveTasks'),
      value: String(snapshot.tasks.liveCount),
    },
  ]

  return (
    <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
      <header className="flex items-baseline justify-between">
        <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.usage.systemSection')}
        </h3>
        <span className="text-xs text-[var(--color-text-tertiary)]">
          {t('settings.usage.systemHint')}
        </span>
      </header>
      <dl className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-x-4 gap-y-2">
        {items.map((item) => (
          <div
            key={item.label}
            className="flex items-baseline justify-between gap-2 border-b border-[var(--color-border)]/20 pb-1 last:border-0"
          >
            <dt className="text-xs uppercase tracking-wide text-[var(--color-text-tertiary)] shrink-0">
              {item.label}
            </dt>
            <dd
              className={`text-xs text-right truncate min-w-0 ${
                item.mono
                  ? 'font-mono text-[var(--color-text-primary)]'
                  : 'text-[var(--color-text-secondary)]'
              }`}
              title={item.value}
            >
              {item.value}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  )
}

function BackgroundTasksSection({
  snapshot,
  t,
}: {
  snapshot: ReturnType<typeof useRuntimeStore.getState>['snapshot']
  t: (key: TranslationKey) => string
}) {
  const groups = snapshot?.tasks.groups ?? []
  const sortedGroups = [...groups].sort(
    (a, b) => b.count - a.count || b.oldestAgeMs - a.oldestAgeMs,
  )

  return (
    <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
      <header className="flex items-baseline justify-between">
        <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.usage.tasksSection')}
        </h3>
        <span className="text-xs text-[var(--color-text-tertiary)]">
          {snapshot
            ? `${snapshot.tasks.liveCount} ${t('settings.usage.tasksLiveSuffix')}`
            : t('settings.usage.systemLoading')}
        </span>
      </header>
      {sortedGroups.length === 0 ? (
        <p className="text-xs text-[var(--color-text-secondary)] py-6 text-center">
          {t('settings.usage.tasksEmpty')}
        </p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-xs">
            <thead className="text-[var(--color-text-tertiary)]">
              <tr className="border-b border-[var(--color-border)]/40">
                <Th>{t('settings.usage.colTaskName')}</Th>
                <Th align="right">{t('settings.usage.colTaskCount')}</Th>
                <Th align="right">{t('settings.usage.colTaskOldest')}</Th>
              </tr>
            </thead>
            <tbody>
              {sortedGroups.map((group) => (
                <tr
                  key={group.name}
                  className="border-b border-[var(--color-border)]/20 last:border-0"
                >
                  <td className="py-2 font-mono text-[var(--color-text-primary)]">
                    {group.name}
                  </td>
                  <td className="py-2 text-right tabular-nums">
                    {formatNumber(group.count)}
                  </td>
                  <td className="py-2 text-right tabular-nums text-[var(--color-text-secondary)]">
                    {formatUptime(Math.round(group.oldestAgeMs / 1000))}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

function CodingModeSection({
  rows,
  totals,
  t,
}: {
  rows: Array<{ id: CodingModeId; stats: UsageCodingModeStats | null }>
  totals: { totalTokens: number; requestCount: number; costUsd: number; activeModes: number }
  t: (key: TranslationKey) => string
}) {
  const sortedRows = useMemo(() => {
    return [...rows].sort((a, b) => {
      const aTokens = a.stats?.totalTokens ?? 0
      const bTokens = b.stats?.totalTokens ?? 0
      return bTokens - aTokens
    })
  }, [rows])

  const maxTokens = useMemo(() => {
    return rows.reduce((acc, r) => Math.max(acc, r.stats?.totalTokens ?? 0), 0)
  }, [rows])

  const noData = totals.totalTokens === 0 && totals.requestCount === 0

  return (
    <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-4">
      <header className="flex items-baseline justify-between">
        <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.usage.codingModeSection')}
        </h3>
        <span className="text-xs text-[var(--color-text-tertiary)]">
          {t('settings.usage.codingModeHint')}
        </span>
      </header>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
        <KpiCard
          label={t('settings.usage.codingModeKpiActive')}
          value={`${totals.activeModes} / ${rows.length}`}
          hint={t('settings.usage.codingModeKpiActiveHint')}
        />
        <KpiCard
          label={t('settings.usage.codingModeKpiRequests')}
          value={formatNumber(totals.requestCount)}
          hint={t('settings.usage.codingModeKpiRequestsHint')}
        />
        <KpiCard
          label={t('settings.usage.codingModeKpiTokens')}
          value={formatNumber(totals.totalTokens)}
          hint={t('settings.usage.codingModeKpiTokensHint')}
        />
        <KpiCard
          label={t('settings.usage.codingModeKpiCost')}
          value={formatUsd(totals.costUsd)}
          hint={t('settings.usage.codingModeKpiCostHint')}
        />
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
        {rows.map(({ id, stats }) => (
          <CodingModeKpi
            key={id}
            id={id}
            stats={stats}
            maxTokens={maxTokens}
            t={t}
          />
        ))}
      </div>

      {noData ? null : (
        <div className="overflow-x-auto">
          <table className="w-full text-xs">
            <thead className="text-[var(--color-text-tertiary)]">
              <tr className="border-b border-[var(--color-border)]/40">
                <Th>{t('settings.usage.colCodingMode')}</Th>
                <Th align="right">{t('settings.usage.colRequests')}</Th>
                <Th align="right">{t('settings.usage.colSessionCount')}</Th>
                <Th align="right">{t('settings.usage.colModelCount')}</Th>
                <Th align="right">{t('settings.usage.colInputTokens')}</Th>
                <Th align="right">{t('settings.usage.colOutputTokens')}</Th>
                <Th align="right">{t('settings.usage.colTotalTokens')}</Th>
                <Th align="right">{t('settings.usage.colCost')}</Th>
                <Th>{t('settings.usage.colLastUsed')}</Th>
              </tr>
            </thead>
            <tbody>
              {sortedRows.map(({ id, stats }) => {
                const label = t(`settings.usage.codingMode.${id}` as TranslationKey)
                const icon = CODING_MODE_ICON[id]
                if (!stats) {
                  return (
                    <tr
                      key={id}
                      className="border-b border-[var(--color-border)]/20 last:border-0"
                    >
                      <td className="py-2">
                        <span className="text-[var(--color-text-tertiary)]">
                          <span className="mr-1">{icon}</span>
                          {label}
                        </span>
                      </td>
                      <td colSpan={8} className="py-2 text-[var(--color-text-tertiary)] italic">
                        {t('settings.usage.codingModeNoData')}
                      </td>
                    </tr>
                  )
                }
                return (
                  <tr
                    key={id}
                    className="border-b border-[var(--color-border)]/20 last:border-0"
                  >
                    <td className="py-2 text-[var(--color-text-primary)]">
                      <span className="mr-1">{icon}</span>
                      {label}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(stats.requestCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(stats.sessionCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(stats.modelCount)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(stats.inputTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums">
                      {formatNumber(stats.outputTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums font-semibold text-[var(--color-text-primary)]">
                      {formatNumber(stats.totalTokens)}
                    </td>
                    <td className="py-2 text-right tabular-nums text-[var(--color-text-secondary)]">
                      {formatUsd(stats.costUsd)}
                    </td>
                    <td className="py-2 text-[var(--color-text-secondary)]">
                      {formatTimestamp(stats.lastUsed)}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

function CodingModeKpi({
  id,
  stats,
  maxTokens,
  t,
}: {
  id: CodingModeId
  stats: UsageCodingModeStats | null
  maxTokens: number
  t: (key: TranslationKey) => string
}) {
  const label = t(`settings.usage.codingMode.${id}` as TranslationKey)
  const icon = CODING_MODE_ICON[id]
  const ratio =
    stats && maxTokens > 0 ? Math.min(1, stats.totalTokens / maxTokens) : 0
  const widthPct = `${Math.round(ratio * 100)}%`
  const inactive = !stats || stats.requestCount === 0

  return (
    <div
      className={`rounded-lg border p-3 ${
        inactive
          ? 'border-[var(--color-border)] bg-[var(--color-surface-container)] opacity-70'
          : 'border-[var(--color-brand)]/40 bg-[var(--color-surface-container)]'
      }`}
    >
      <div className="flex items-baseline justify-between gap-2">
        <p className="text-xs text-[var(--color-text-tertiary)] uppercase tracking-wide truncate">
          <span className="mr-1">{icon}</span>
          {label}
        </p>
        <span className="text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
          {stats ? `${formatNumber(stats.requestCount)} req` : '—'}
        </span>
      </div>
      <p className="mt-1 text-xs font-semibold text-[var(--color-text-primary)] tabular-nums">
        {stats ? formatNumber(stats.totalTokens) : '0'}
      </p>
      <p className="text-xs text-[var(--color-text-secondary)] mt-0.5">
        {stats ? formatUsd(stats.costUsd) : t('settings.usage.codingModeNoData')}
      </p>
      <div className="mt-2 h-1.5 rounded-full bg-[var(--color-border)]/40 overflow-hidden">
        <div
          className="h-full rounded-full bg-[var(--color-brand)]/70"
          style={{ width: widthPct }}
        />
      </div>
    </div>
  )
}

function KpiCard({
  label,
  value,
  hint,
  emphasis,
}: {
  label: string
  value: string
  hint?: string
  emphasis?: boolean
}) {
  return (
    <div
      className={`rounded-lg border p-3 ${
        emphasis
          ? 'border-[var(--color-brand)]/40 bg-[var(--color-surface-container)]'
          : 'border-[var(--color-border)] bg-[var(--color-surface-container)]'
      }`}
    >
      <p className="text-xs text-[var(--color-text-tertiary)] uppercase tracking-wide">
        {label}
      </p>
      <p
        className={`mt-1 font-semibold text-[var(--color-text-primary)] tabular-nums ${
          emphasis ? 'text-xs' : 'text-xs'
        }`}
      >
        {value}
      </p>
      {hint && (
        <p className="text-xs text-[var(--color-text-secondary)] mt-1">
          {hint}
        </p>
      )}
    </div>
  )
}

function Th({
  children,
  align = 'left',
}: {
  children: React.ReactNode
  align?: 'left' | 'right'
}) {
  return (
    <th
      className={`py-2 px-1 text-xs uppercase tracking-wide font-normal ${
        align === 'right' ? 'text-right' : 'text-left'
      }`}
    >
      {children}
    </th>
  )
}
