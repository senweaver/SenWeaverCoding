// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useTranslation } from '../../i18n'

type Props = {
  selected: number[]
  onChange: (days: number[]) => void
}

const DAY_ORDER = [1, 2, 3, 4, 5, 6, 0]

const DAY_KEYS = [
  'newTask.daySun',
  'newTask.dayMon',
  'newTask.dayTue',
  'newTask.dayWed',
  'newTask.dayThu',
  'newTask.dayFri',
  'newTask.daySat',
] as const

export function DayOfWeekPicker({ selected, onChange }: Props) {
  const t = useTranslation()

  const toggle = (day: number) => {
    if (selected.includes(day)) {
      if (selected.length <= 1) return
      onChange(selected.filter((d) => d !== day))
    } else {
      onChange([...selected, day])
    }
  }

  return (
    <div className="flex gap-1.5">
      {DAY_ORDER.map((day) => {
        const isActive = selected.includes(day)
        return (
          <button
            key={day}
            type="button"
            onClick={() => toggle(day)}
            className={`
              w-7 h-7 rounded-lg text-xs font-semibold transition-colors
              ${isActive
                ? 'bg-[var(--color-brand)] text-[var(--color-on-primary)] border border-[var(--color-brand)]'
                : 'bg-[var(--color-surface)] text-[var(--color-text-secondary)] border border-[var(--color-border)] hover:bg-[var(--color-surface-hover)]'
              }
            `}
          >
            {t(DAY_KEYS[day]!)}
          </button>
        )
      })}
    </div>
  )
}
