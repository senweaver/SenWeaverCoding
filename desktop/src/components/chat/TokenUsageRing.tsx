import { useMemo } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useSessionRuntimeStore, DRAFT_RUNTIME_SELECTION_KEY } from '../../stores/sessionRuntimeStore'
import { useSettingsStore } from '../../stores/settingsStore'
import { useProviderStore } from '../../stores/providerStore'
import { useTranslation } from '../../i18n'
import { Popover } from '../shared/Popover'
import {
  DEFAULT_CONTEXT_WINDOW,
  formatTokenCount,
  resolveContextWindow,
} from '../../utils/contextWindow'

type Props = {

  sessionId?: string | null

  size?: number
}

export function TokenUsageRing({ sessionId, size = 16 }: Props) {
  const t = useTranslation()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const targetSessionId = sessionId === undefined ? activeTabId : sessionId
  const cumulativeTokens = useChatStore((s) =>
    targetSessionId ? s.sessions[targetSessionId]?.cumulativeTokens ?? 0 : 0,
  )

  const runtimeSelection = useSessionRuntimeStore((s) =>
    targetSessionId
      ? s.selections[targetSessionId] ?? s.selections[DRAFT_RUNTIME_SELECTION_KEY]
      : s.selections[DRAFT_RUNTIME_SELECTION_KEY],
  )
  const settingsModel = useSettingsStore((s) => s.currentModel?.id ?? null)
  const modelId = runtimeSelection?.modelId ?? settingsModel ?? null

  const providerId = runtimeSelection?.providerId ?? null
  const providers = useProviderStore((s) => s.providers)
  const activeProviderId = useProviderStore((s) => s.activeId)
  const overrideTokens = useMemo(() => {
    const targetProviderId = providerId ?? activeProviderId
    if (!targetProviderId || !modelId) return null
    const provider = providers.find((p) => p.id === targetProviderId)
    const value = provider?.modelContextWindows?.[modelId]
    return typeof value === 'number' && Number.isFinite(value) && value > 0
      ? value
      : null
  }, [providers, providerId, activeProviderId, modelId])

  const { used, total, pct } = useMemo(() => {
    const limit = resolveContextWindow(modelId, overrideTokens) || DEFAULT_CONTEXT_WINDOW
    const safeCumulative = Math.max(0, cumulativeTokens)
    if (limit <= 0) {
      return { used: safeCumulative, total: limit, pct: 0 }
    }

    const inCycle = safeCumulative % limit
    const fraction = Math.max(0, Math.min(1, inCycle / limit))
    return {
      used: inCycle,
      total: limit,
      pct: fraction,
    }
  }, [modelId, cumulativeTokens, overrideTokens])

  const stroke = 1.6
  const radius = (size - stroke) / 2
  const cx = size / 2
  const cy = size / 2
  const circumference = 2 * Math.PI * radius
  const dash = circumference * pct

  const arcColor =
    pct >= 0.9
      ? 'var(--color-error)'
      : pct >= 0.7
        ? 'var(--color-warning)'
        : 'var(--color-text-secondary)'

  const pctLabel = (pct * 100).toFixed(1).replace(/\.0$/, '')
  const tooltipText = t('chat.tokenRing.tooltip', {
    pct: pctLabel,
    used: formatTokenCount(used),
    total: formatTokenCount(total),
  })

  return (
    <Popover
      trigger="hover"
      placement="top"
      minWidth={0}
      maxWidth={320}
      panelClassName="px-2.5 py-1.5"
      content={
        <span className="whitespace-nowrap text-[11px] text-[var(--color-text-primary)]">
          {tooltipText}
        </span>
      }
    >
      <button
        type="button"
        aria-label={tooltipText}
        title=""
        className="relative flex h-6 w-6 flex-shrink-0 items-center justify-center rounded-full text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
      >
        <svg
          width={size}
          height={size}
          viewBox={`0 0 ${size} ${size}`}
          fill="none"
          aria-hidden
        >
          {}
          <circle
            cx={cx}
            cy={cy}
            r={radius}
            stroke="var(--color-border)"
            strokeWidth={stroke}
          />
          {}
          {pct > 0 && (
            <circle
              cx={cx}
              cy={cy}
              r={radius}
              stroke={arcColor}
              strokeWidth={stroke}
              strokeDasharray={`${dash} ${circumference - dash}`}
              strokeLinecap="round"
              transform={`rotate(-90 ${cx} ${cy})`}
            />
          )}
        </svg>
      </button>
    </Popover>
  )
}
