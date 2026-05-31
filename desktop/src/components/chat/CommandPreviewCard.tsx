// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useTranslation } from '../../i18n'
import { getCategoryIcon, getToolCategory } from '../../utils/toolCategory'
import { extractCommand, firstWord, truncate } from '../../utils/toolFormatters'

type Props = {
  toolName: string
  input: unknown
}

export function CommandPreviewCard({ toolName, input }: Props) {
  const t = useTranslation()
  const category = getToolCategory(toolName)
  const icon = getCategoryIcon(category)
  const command = extractCommand(input)

  return (
    <div className="mb-2 overflow-hidden rounded-lg border border-dashed border-[var(--color-outline)]/30 bg-[var(--color-surface-container-lowest)]/60 px-3 py-1.5">
      <div className="flex items-center gap-2">
        <span className="material-symbols-outlined shrink-0 text-[13px] text-[var(--color-outline)]">
          {icon}
        </span>
        <span className="text-[11px] font-medium text-[var(--color-text-tertiary)]">
          {t('tool.aboutToRun')}
        </span>
        <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
          {command ? `${firstWord(command)} ${truncate(command.replace(/\s+/g, ' '), 80)}` : toolName}
        </span>
      </div>
    </div>
  )
}
