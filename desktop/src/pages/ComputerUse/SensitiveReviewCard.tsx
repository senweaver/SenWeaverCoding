// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useTranslation } from '../../i18n'
import type { SensitiveReport } from '../../api/computer'

export function SensitiveReviewCard({ report }: { report: SensitiveReport }) {
  const t = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const frames = report.images?.framesBlurred ?? 0
  const regions = report.images?.regionsBlurred ?? 0

  const summary =
    frames > 0
      ? t('computerUse.sensitive.summaryWithFrames', {
          findings: report.totalFindings,
          regions,
        })
      : t('computerUse.sensitive.summary', { findings: report.totalFindings })

  return (
    <div className="mb-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] p-3">
      <div className="flex items-center gap-2">
        <span className="material-symbols-outlined text-[16px] text-[var(--color-brand)]">
          shield
        </span>
        <span className="text-[12px] text-[var(--color-text-primary)]">{summary}</span>
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="ml-auto text-[11px] text-[var(--color-brand)] hover:opacity-80"
        >
          {expanded ? t('computerUse.sensitive.hide') : t('computerUse.sensitive.review')}
        </button>
      </div>
      {expanded && (
        <div className="mt-2 flex flex-col gap-1.5">
          {report.findings.map((finding, idx) => (
            <div
              key={`${finding.source}-${finding.redactedValue}-${idx}`}
              className="flex items-start gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5"
            >
              <span
                className={`mt-1 h-2 w-2 shrink-0 rounded-full ${
                  finding.severity === 'high'
                    ? 'bg-[var(--color-error)]'
                    : finding.severity === 'medium'
                      ? 'bg-[var(--color-warning)]'
                      : 'bg-[var(--color-text-tertiary)]'
                }`}
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5 text-[11px]">
                  <span className="font-medium text-[var(--color-text-primary)]">
                    {finding.label}
                  </span>
                  <span className="text-[var(--color-text-tertiary)]">· {finding.source}</span>
                  {finding.occurrences > 1 && (
                    <span className="text-[var(--color-text-tertiary)]">×{finding.occurrences}</span>
                  )}
                </div>
                <div className="truncate font-mono text-[10px] text-[var(--color-text-secondary)]">
                  {finding.snippet}
                </div>
              </div>
            </div>
          ))}
          {frames > 0 && (
            <div className="text-[10px] italic text-[var(--color-text-tertiary)]">
              {t('computerUse.sensitive.framesNote')}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export default SensitiveReviewCard
