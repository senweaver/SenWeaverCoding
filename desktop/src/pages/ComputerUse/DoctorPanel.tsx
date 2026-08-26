// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { getDoctor, type DoctorReport } from '../../api/computer'

export function DoctorPanel({ onClose }: { onClose: () => void }) {
  const t = useTranslation()
  const [report, setReport] = useState<DoctorReport | null>(null)

  useEffect(() => {
    let alive = true
    void getDoctor()
      .then((r) => {
        if (alive) setReport(r)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  const row = (label: string, ok: boolean, okText: string, badText: string) => (
    <div className="flex items-center justify-between border-b border-[var(--color-border)] py-2 last:border-0">
      <span className="text-[12px] text-[var(--color-text-primary)]">{label}</span>
      <span
        className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium ${
          ok
            ? 'bg-[var(--color-success)]/12 text-[var(--color-success)]'
            : 'bg-[var(--color-warning)]/15 text-[var(--color-warning)]'
        }`}
      >
        <span className="material-symbols-outlined text-[12px]">
          {ok ? 'check_circle' : 'error'}
        </span>
        {ok ? okText : badText}
      </span>
    </div>
  )

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/40 p-6" onClick={onClose}>
      <div
        className="w-[min(420px,94vw)] overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
          <span className="material-symbols-outlined text-[18px] text-[var(--color-brand)]">
            health_and_safety
          </span>
          <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
            {t('computerUse.doctor.title')}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
          >
            <span className="material-symbols-outlined text-[16px]">close</span>
          </button>
        </div>
        <div className="px-4 py-2">
          {report ? (
            <>
              {row(
                t('computerUse.doctor.recording'),
                report.recordingSupported,
                t('computerUse.doctor.available'),
                t('computerUse.doctor.unavailable'),
              )}
              {row(
                t('computerUse.doctor.vision'),
                report.visionModelCount > 0,
                t('computerUse.doctor.available'),
                t('computerUse.doctor.unavailable'),
              )}
              {row(
                t('computerUse.doctor.transcription'),
                report.transcriptionConfigured,
                t('computerUse.doctor.configured'),
                t('computerUse.doctor.notConfigured'),
              )}
              {row(
                t('computerUse.doctor.ocr'),
                report.ocrAvailable,
                t('computerUse.doctor.available'),
                t('computerUse.doctor.unavailable'),
              )}
            </>
          ) : (
            <div className="py-6 text-center text-[12px] text-[var(--color-text-secondary)]">…</div>
          )}
        </div>
      </div>
    </div>
  )
}

export default DoctorPanel
