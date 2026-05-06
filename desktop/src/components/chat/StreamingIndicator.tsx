import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'

function formatElapsed(seconds: number): string {
  if (seconds < 60) return `${seconds}s`
  const m = Math.floor(seconds / 60)
  const s = seconds % 60
  return `${m}m ${s}s`
}

export function StreamingIndicator() {
  const t = useTranslation()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const elapsedSeconds = useChatStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.elapsedSeconds ?? 0 : 0,
  )

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
      {elapsedSeconds > 0 && (
        <span className="text-[11px] tabular-nums text-[var(--color-text-tertiary)]/70">
          {formatElapsed(elapsedSeconds)}
        </span>
      )}
    </div>
  )
}
