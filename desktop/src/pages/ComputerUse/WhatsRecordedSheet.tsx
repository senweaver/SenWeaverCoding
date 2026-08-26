// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useTranslation } from '../../i18n'

export function WhatsRecordedSheet({ onClose }: { onClose: () => void }) {
  const t = useTranslation()
  const rows: { icon: string; text: string }[] = [
    { icon: 'ads_click', text: t('computerUse.whatsRecorded.activity') },
    { icon: 'photo_camera', text: t('computerUse.whatsRecorded.frames') },
    { icon: 'mic', text: t('computerUse.whatsRecorded.narration') },
    { icon: 'folder', text: t('computerUse.whatsRecorded.storage') },
    { icon: 'cloud_upload', text: t('computerUse.whatsRecorded.analyze') },
    { icon: 'shield', text: t('computerUse.whatsRecorded.protection') },
  ]
  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/50 p-6" onClick={onClose}>
      <div
        className="w-[min(460px,94vw)] overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
          <span className="material-symbols-outlined text-[18px] text-[var(--color-brand)]">
            visibility
          </span>
          <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
            {t('computerUse.whatsRecorded.title')}
          </div>
        </div>
        <div className="flex flex-col gap-2.5 px-4 py-3">
          {rows.map((row, idx) => (
            <div key={idx} className="flex items-start gap-2">
              <span className="material-symbols-outlined mt-0.5 text-[16px] text-[var(--color-text-tertiary)]">
                {row.icon}
              </span>
              <span className="text-[12px] leading-relaxed text-[var(--color-text-secondary)]">
                {row.text}
              </span>
            </div>
          ))}
        </div>
        <div className="flex justify-end border-t border-[var(--color-border)] px-4 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md bg-[var(--color-brand)] px-3 py-1.5 text-[11px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90"
          >
            {t('computerUse.whatsRecorded.got')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default WhatsRecordedSheet
