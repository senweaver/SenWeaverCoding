import { useTranslation } from '../../i18n'

type AnswerItem = {
  question: string
  answer: string | string[]
}

type Props = {
  items: AnswerItem[]
  details?: string
  superseded?: boolean
}

function isAnswerEmpty(answer: string | string[]): boolean {
  if (Array.isArray(answer)) return answer.length === 0
  return !answer
}

export function AnswersCard({ items, details, superseded }: Props) {
  const t = useTranslation()
  if (items.length === 0 && !details) return null

  return (
    <div
      className={`mb-3 ${superseded ? 'opacity-60 saturate-50 pointer-events-none' : ''}`}
    >
      <div className="rounded-[var(--radius-lg)] border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] overflow-hidden">
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-text-secondary)]">
            quickreply
          </span>
          <span className="text-[12px] font-semibold text-[var(--color-text-primary)]">
            {t('plan.answersTitle')}
          </span>
        </div>
        <div className="px-3 py-2 space-y-2">
          {items.map((item, idx) => {
            const empty = isAnswerEmpty(item.answer)
            return (
              <div key={idx} className="flex flex-col gap-0.5">
                <div className="text-[12px] font-medium text-[var(--color-text-secondary)] leading-snug">
                  {item.question}
                </div>
                <div className="text-[12px] text-[var(--color-text-primary)] leading-snug pl-2 border-l-2 border-[var(--color-plan-accent)]/40">
                  {empty ? (
                    <em className="text-[var(--color-text-tertiary)]">
                      {t('plan.skippedAnswer')}
                    </em>
                  ) : Array.isArray(item.answer) ? (
                    <span className="flex flex-wrap gap-1">
                      {item.answer.map((label, j) => (
                        <span
                          key={j}
                          className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] bg-[var(--color-plan-accent-container)] text-[var(--color-on-plan-accent-container)]"
                        >
                          <span className="material-symbols-outlined text-[12px]">check</span>
                          {label}
                        </span>
                      ))}
                    </span>
                  ) : (
                    item.answer
                  )}
                </div>
              </div>
            )
          })}
          {details && (
            <div className="pt-1.5 border-t border-[var(--color-outline-variant)]/20 text-[11px] text-[var(--color-text-tertiary)] leading-snug">
              {details}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
