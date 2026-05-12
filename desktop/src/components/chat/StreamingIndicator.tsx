import { useTranslation } from '../../i18n'

export function StreamingIndicator() {
  const t = useTranslation()

  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="planning-indicator"
      className="mb-3 flex w-fit items-center gap-2 px-1 py-0.5 text-[var(--color-text-tertiary)]"
    >
      <span
        aria-hidden="true"
        className="size-1.5 flex-shrink-0 rounded-full bg-[var(--color-text-tertiary)] animate-pulse"
      />
      <span className="text-sm italic">{t('chat.planningNextMoves')}</span>
    </div>
  )
}
