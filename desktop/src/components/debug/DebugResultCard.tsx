// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, useState } from 'react'
import { useSettingsStore } from '../../stores/settingsStore'
import { useChatStore } from '../../stores/chatStore'
import { debugApi, type DebugReport } from '../../api/debug'

export function DebugResultCard({ sessionId }: { sessionId: string }) {
  const locale = useSettingsStore((s) => s.locale)
  const globalCodingMode = useSettingsStore((s) => s.codingMode)
  const sessionCodingMode = useChatStore((s) => s.sessionCodingMode[sessionId])
  const codingMode = sessionCodingMode ?? globalCodingMode
  const chatState = useChatStore((s) => s.sessions[sessionId]?.chatState ?? 'idle')

  const [report, setReport] = useState<DebugReport | null>(null)
  const [dismissed, setDismissed] = useState(false)
  const prevChatState = useRef(chatState)

  const isZh = locale === 'zh'
  const tr = (en: string, zh: string) => (isZh ? zh : en)

  const fetchReport = () => {
    void debugApi
      .report(sessionId)
      .then((res) => {
        if (res.report) {
          setReport((prev) => {
            if (prev && prev.runId === res.report!.runId) return prev
            setDismissed(false)
            return res.report
          })
        }
      })
      .catch(() => {})
  }

  useEffect(() => {
    setReport(null)
    setDismissed(false)
  }, [sessionId])

  useEffect(() => {
    if (codingMode === 'debug') fetchReport()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, codingMode])

  useEffect(() => {
    const prev = prevChatState.current
    prevChatState.current = chatState
    if (prev !== 'idle' && chatState === 'idle' && codingMode === 'debug') {
      fetchReport()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatState, codingMode])

  if (codingMode !== 'debug' || !report || dismissed) return null

  const f = report.summary.findings
  const c = report.summary.cases
  const submodeLabel = SUBMODE_LABELS[report.submode] ?? {
    en: report.submode,
    zh: report.submode,
  }

  return (
    <div className="mx-auto w-full max-w-[860px] px-4 pb-2">
      <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-4 py-3 shadow-[var(--shadow-dropdown)]">
        <div className="flex items-start justify-between gap-2">
          <div className="flex items-center gap-2 min-w-0">
            <span className="material-symbols-outlined text-[18px] text-[var(--color-accent)]">
              fact_check
            </span>
            <div className="min-w-0">
              <div className="truncate text-[13px] font-semibold text-[var(--color-text-primary)]">
                {report.title}
              </div>
              <div className="text-[11px] text-[var(--color-text-secondary)]">
                {tr('Debug report', '调试报告')} · {tr(submodeLabel.en, submodeLabel.zh)}
              </div>
            </div>
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={fetchReport}
              title={tr('Refresh', '刷新')}
              className="flex h-6 w-6 items-center justify-center rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
            >
              <span className="material-symbols-outlined text-[15px]">refresh</span>
            </button>
            <button
              type="button"
              onClick={() => setDismissed(true)}
              title={tr('Dismiss', '关闭')}
              className="flex h-6 w-6 items-center justify-center rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
            >
              <span className="material-symbols-outlined text-[15px]">close</span>
            </button>
          </div>
        </div>

        <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
          <Stat label={tr('Findings', '问题')} value={f.total} />
          {f.p0 > 0 && <Badge tone="error" label={`P0 ${f.p0}`} />}
          {f.p1 > 0 && <Badge tone="warning" label={`P1 ${f.p1}`} />}
          {f.p2 > 0 && <Badge tone="muted" label={`P2 ${f.p2}`} />}
          {c.total > 0 && (
            <>
              <span className="mx-0.5 h-3 w-px bg-[var(--color-border)]" />
              {c.passed > 0 && <Badge tone="success" label={`${tr('Pass', '通过')} ${c.passed}`} />}
              {c.failed > 0 && <Badge tone="error" label={`${tr('Fail', '失败')} ${c.failed}`} />}
              {c.blocked > 0 && (
                <Badge tone="warning" label={`${tr('Blocked', '阻塞')} ${c.blocked}`} />
              )}
            </>
          )}
          {report.summary.coverage > 0 && (
            <Stat label={tr('Pages', '页面')} value={report.summary.coverage} />
          )}
        </div>

        {report.findings.length > 0 && (
          <div className="mt-2.5 space-y-1">
            {report.findings.slice(0, 5).map((finding) => (
              <div
                key={finding.id}
                className="flex items-start gap-2 rounded-md bg-[var(--color-surface)] px-2.5 py-1.5"
              >
                <span
                  className={`mt-0.5 inline-flex h-4 shrink-0 items-center rounded px-1 text-[10px] font-semibold ${bucketClass(
                    finding.bucket,
                  )}`}
                >
                  {finding.bucket.toUpperCase()}
                </span>
                <div className="min-w-0">
                  <div className="truncate text-[12px] text-[var(--color-text-primary)]">
                    {finding.title}
                  </div>
                  {finding.category && (
                    <div className="text-[10px] uppercase tracking-wide text-[var(--color-text-tertiary)]">
                      {finding.category}
                    </div>
                  )}
                </div>
              </div>
            ))}
            {report.findings.length > 5 && (
              <div className="px-1 text-[11px] text-[var(--color-text-secondary)]">
                {tr(
                  `+${report.findings.length - 5} more`,
                  `还有 ${report.findings.length - 5} 项`,
                )}
              </div>
            )}
          </div>
        )}

        <div className="mt-2.5 flex flex-wrap gap-3 text-[11px] text-[var(--color-text-secondary)]">
          {report.artifacts.report && (
            <span className="font-[var(--font-mono)]">{report.artifacts.report}</span>
          )}
        </div>
      </div>
    </div>
  )
}

const SUBMODE_LABELS: Record<string, { en: string; zh: string }> = {
  auto: { en: 'Auto', zh: '自动' },
  'code-review': { en: 'Code Review', zh: '代码审查' },
  'security-review': { en: 'Security Review', zh: '安全审查' },
  e2e: { en: 'E2E Testing', zh: '端到端测试' },
  performance: { en: 'Performance & Load', zh: '性能与负载' },
}

function bucketClass(bucket: string): string {
  switch (bucket) {
    case 'p0':
      return 'bg-[var(--color-error-container)] text-[var(--color-error)]'
    case 'p1':
      return 'bg-[var(--color-surface)] text-[var(--color-warning)]'
    default:
      return 'bg-[var(--color-surface-container-low)] text-[var(--color-text-secondary)]'
  }
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-[var(--color-surface)] px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)]">
      <span className="font-semibold text-[var(--color-text-primary)]">{value}</span>
      <span>{label}</span>
    </span>
  )
}

function Badge({
  label,
  tone,
}: {
  label: string
  tone: 'error' | 'warning' | 'success' | 'muted'
}) {
  const cls =
    tone === 'error'
      ? 'bg-[var(--color-error-container)] text-[var(--color-error)]'
      : tone === 'warning'
        ? 'bg-[var(--color-surface)] text-[var(--color-warning)]'
        : tone === 'success'
          ? 'bg-[var(--color-surface)] text-[var(--color-success)]'
          : 'bg-[var(--color-surface)] text-[var(--color-text-secondary)]'
  return (
    <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium ${cls}`}>
      {label}
    </span>
  )
}
