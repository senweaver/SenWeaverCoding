// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useRef, type KeyboardEvent } from 'react'

export type SegmentedOption<T extends string> = {
  value: T
  label: string
  disabled?: boolean
}

type SegmentedControlProps<T extends string> = {
  value: T
  options: ReadonlyArray<SegmentedOption<T>>
  onChange: (value: T) => void
  ariaLabel: string
  disabled?: boolean
  minItemWidth?: number
  className?: string
}

export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
  disabled = false,
  minItemWidth = 88,
  className = '',
}: SegmentedControlProps<T>) {
  const groupRef = useRef<HTMLDivElement | null>(null)

  const focusOption = (nextValue: T) => {
    const el = groupRef.current?.querySelector<HTMLButtonElement>(
      `[data-segment-value="${nextValue}"]`,
    )
    el?.focus()
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return
    const enabled = options.filter((o) => !o.disabled)
    if (enabled.length === 0) return
    const currentIndex = Math.max(
      0,
      enabled.findIndex((o) => o.value === value),
    )
    let nextIndex: number | null = null
    switch (event.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        nextIndex = (currentIndex + 1) % enabled.length
        break
      case 'ArrowLeft':
      case 'ArrowUp':
        nextIndex = (currentIndex - 1 + enabled.length) % enabled.length
        break
      case 'Home':
        nextIndex = 0
        break
      case 'End':
        nextIndex = enabled.length - 1
        break
      default:
        return
    }
    event.preventDefault()
    const next = enabled[nextIndex]
    if (!next || next.value === value) return
    onChange(next.value)
    focusOption(next.value)
  }

  return (
    <div
      ref={groupRef}
      role="radiogroup"
      aria-label={ariaLabel}
      aria-disabled={disabled || undefined}
      onKeyDown={handleKeyDown}
      className={`flex flex-wrap gap-2 ${className}`}
    >
      {options.map((option) => {
        const selected = option.value === value
        const itemDisabled = disabled || option.disabled === true
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            data-segment-value={option.value}
            tabIndex={selected ? 0 : -1}
            disabled={itemDisabled}
            onClick={() => {
              if (!selected) onChange(option.value)
            }}
            style={{ minWidth: minItemWidth }}
            className={`h-7 rounded-lg border px-4 text-xs font-semibold transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-brand)]/40 focus-visible:ring-offset-1 focus-visible:ring-offset-[var(--color-surface)] disabled:cursor-not-allowed disabled:opacity-50 ${
              selected
                ? 'border-transparent bg-[image:var(--gradient-btn-primary)] text-[var(--color-btn-primary-fg)] shadow-[var(--shadow-button-primary)]'
                : 'border-[var(--color-border)] bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
            }`}
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}
