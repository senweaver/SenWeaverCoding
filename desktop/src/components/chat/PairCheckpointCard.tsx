import { useTranslation } from '../../i18n'

type Props = {
  superseded?: boolean
}

export function PairCheckpointCard({ superseded }: Props) {
  const t = useTranslation()

  return (
    <div
      className={`mb-3 ${superseded ? 'opacity-60 saturate-50 pointer-events-none' : ''}`}
    >
      <div className="rounded-[var(--radius-lg)] border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] overflow-hidden">
        <div className="flex items-center gap-2 px-3 py-2">
          <span
            className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)]"
            style={{ fontVariationSettings: "'FILL' 1" }}
          >
            pause_circle
          </span>
          <span className="text-[12px] font-semibold text-[var(--color-text-primary)]">
            {t('pairCheckpoint.title')}
          </span>
          <span className="ml-auto inline-flex items-center gap-1 rounded-full bg-[var(--color-surface-container)] px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-[var(--color-text-tertiary)]">
            <span className="material-symbols-outlined text-[12px]">timer</span>
            {t('pairCheckpoint.awaiting')}
          </span>
        </div>
        <div className="px-3 pb-2 text-[12px] text-[var(--color-text-secondary)] leading-relaxed">
          {t('pairCheckpoint.body')}
        </div>
        <div className="flex items-center justify-between gap-2 border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)] px-3 py-1.5">
          <span className="flex items-center gap-1 text-[11px] text-[var(--color-text-tertiary)]">
            <span className="material-symbols-outlined text-[12px]">keyboard</span>
            {t('pairCheckpoint.hint')}
          </span>
        </div>
      </div>
    </div>
  )
}
