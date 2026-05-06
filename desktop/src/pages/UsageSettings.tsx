import { useEffect, useMemo } from 'react'
import { useTranslation } from '../i18n'
import { Button } from '../components/shared/Button'
import { useUsageStore } from '../stores/usageStore'
import { useSessionStore } from '../stores/sessionStore'
import { resolveSessionTitle } from '../utils/sessionTitle'
import type { SessionListItem } from '../types/session'
import type { UsageLifetimeStats, UsageSessionStats } from '../types/usage'

function formatUsd(value: number): string {
  if (!Number.isFinite(value)) return '$0.00'
  if (value >= 1) return `$${value.toFixed(2)}`
  if (value > 0) return `$${value.toFixed(4)}`
  return '$0.00'
}

function formatNumber(value: number): string {
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
  const fetch = useUsageStore((s) => s.fetch)
  const sessions = useSessionStore((s) => s.sessions)

  useEffect(() => {
    void fetch()
  }, [fetch])

  const lifetimeRows = useMemo<UsageLifetimeStats[]>(() => {
    if (!summary) return []
    return Object.values(summary.byModelLifetime).sort(
      (a, b) => b.totalTokens - a.totalTokens || b.costUsd - a.costUsd,
    )
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

  return (
    <div className="max-w-5xl space-y-6">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.title')}
          </h2>
          <p className="text-xs text-[var(--color-text-secondary)] mt-1">
            {t('settings.usage.description')}
          </p>
        </div>
        <Button variant="secondary" onClick={() => void fetch()} disabled={isLoading}>
          {isLoading ? t('common.loading') : t('common.refresh')}
        </Button>
      </div>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      {}
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
      </div>

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-4 space-y-3">
        <header className="flex items-baseline justify-between">
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.lifetimeSection')}
          </h3>
          <span className="text-[11px] text-[var(--color-text-tertiary)]">
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

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-4 space-y-3">
        <header className="flex items-baseline justify-between">
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('settings.usage.sessionSection')}
          </h3>
          <span className="text-[11px] text-[var(--color-text-tertiary)]">
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
      <p className="text-[11px] text-[var(--color-text-tertiary)] uppercase tracking-wide">
        {label}
      </p>
      <p
        className={`mt-1 font-semibold text-[var(--color-text-primary)] tabular-nums ${
          emphasis ? 'text-2xl' : 'text-xl'
        }`}
      >
        {value}
      </p>
      {hint && (
        <p className="text-[11px] text-[var(--color-text-secondary)] mt-1">
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
      className={`py-2 px-1 text-[11px] uppercase tracking-wide font-normal ${
        align === 'right' ? 'text-right' : 'text-left'
      }`}
    >
      {children}
    </th>
  )
}
