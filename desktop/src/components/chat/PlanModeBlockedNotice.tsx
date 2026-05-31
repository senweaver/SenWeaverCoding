// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useTranslation } from '../../i18n'
import { CODING_MODE_ACCENT } from '../../types/codingMode'
import type { CodingModeId } from '../../types/codingMode'

type BlockedTool = {
  name: string
  input?: unknown
}

export type ModeBlockedReason = 'plan' | 'read_only' | 'tool_not_allowed'

type Props = {
  tools: BlockedTool[]
  superseded?: boolean
  mode?: CodingModeId
  reason?: ModeBlockedReason
  detail?: string
}

export function ModeBlockedNotice({
  tools,
  superseded,
  mode = 'plan',
  reason = 'plan',
  detail,
}: Props) {
  const t = useTranslation()
  if (tools.length === 0 && !detail) return null

  const uniqueNames = Array.from(new Set(tools.map((tool) => tool.name)))
  const toolList = uniqueNames.join(', ')

  const accent =
    CODING_MODE_ACCENT[mode] ?? CODING_MODE_ACCENT.plan ?? CODING_MODE_ACCENT.agent
  const accentColor = accent?.accent ?? 'var(--color-text-secondary)'
  const accentBg = accent?.container ?? 'var(--color-surface-container)'
  const accentFg = accent?.onContainer ?? 'var(--color-text-primary)'

  const titleKey =
    reason === 'read_only'
      ? 'modeBlocked.readOnly.title'
      : reason === 'tool_not_allowed'
        ? 'modeBlocked.toolNotAllowed.title'
        : 'plan.blockedTitle'
  const bodyKey =
    reason === 'read_only'
      ? 'modeBlocked.readOnly.body'
      : reason === 'tool_not_allowed'
        ? 'modeBlocked.toolNotAllowed.body'
        : 'plan.blockedBody'

  const titleResolved = (t(titleKey as never) as string) || titleKey
  const bodyResolved = (t(bodyKey as never, { tools: toolList } as never) as string) || ''

  return (
    <div
      className={`mb-3 ${superseded ? 'opacity-60 saturate-50 pointer-events-none' : ''}`}
    >
      <div
        className="rounded-[var(--radius-md)] border px-3 py-2 flex items-start gap-2"
        style={{
          borderColor: `${accentColor}66`,
          backgroundColor: accentBg,
          color: accentFg,
        }}
      >
        <span
          className="material-symbols-outlined text-[16px] leading-[18px] shrink-0"
          style={{ color: accentColor }}
        >
          shield
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-[12px] font-semibold">{titleResolved}</div>
          {bodyResolved && (
            <div className="mt-0.5 text-[11px] leading-snug text-[var(--color-text-secondary)]">
              {bodyResolved}
            </div>
          )}
          {detail && (
            <div className="mt-1 text-[11px] leading-snug text-[var(--color-text-secondary)] whitespace-pre-wrap break-words">
              {detail}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

export const PlanModeBlockedNotice = ModeBlockedNotice
