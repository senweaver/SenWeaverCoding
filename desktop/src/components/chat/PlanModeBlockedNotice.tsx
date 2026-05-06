import { useTranslation } from '../../i18n'

type BlockedTool = {
  name: string
  input?: unknown
}

type Props = {
  tools: BlockedTool[]
  superseded?: boolean
}

export function PlanModeBlockedNotice({ tools, superseded }: Props) {
  const t = useTranslation()
  if (tools.length === 0) return null

  const uniqueNames = Array.from(new Set(tools.map((tool) => tool.name)))
  const toolList = uniqueNames.join(', ')

  return (
    <div
      className={`mb-3 ${superseded ? 'opacity-60 saturate-50 pointer-events-none' : ''}`}
    >
      <div className="rounded-[var(--radius-md)] border border-[var(--color-plan-accent)]/40 bg-[var(--color-plan-accent)]/8 px-3 py-2 flex items-start gap-2">
        <span className="material-symbols-outlined text-[16px] leading-[18px] text-[var(--color-plan-accent)] shrink-0">
          shield
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[12px] font-semibold text-[var(--color-on-plan-accent-container)]">
            {t('plan.blockedTitle')}
          </div>
          <div className="mt-0.5 text-[11px] leading-snug text-[var(--color-text-secondary)]">
            {t('plan.blockedBody', { tools: toolList })}
          </div>
        </div>
      </div>
    </div>
  )
}
