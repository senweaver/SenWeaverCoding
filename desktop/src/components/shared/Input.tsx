// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { InputHTMLAttributes } from 'react'

type InputProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'size'> & {
  label?: string
  error?: string
  required?: boolean
  size?: 'sm' | 'md'
}

export function Input({ label, error, required, size = 'md', className = '', id, ...props }: InputProps) {
  const inputId = id || label?.toLowerCase().replace(/\s+/g, '-')
  const compact = size === 'sm'
  return (
    <div className="flex flex-col gap-1">
      {label && (
        <label htmlFor={inputId} className={`${compact ? 'text-xs' : 'text-sm'} font-medium text-[var(--color-text-primary)]`}>
          {label}
          {required && <span className="text-[var(--color-error)] ml-0.5">*</span>}
        </label>
      )}
      <input
        id={inputId}
        className={`
          ${compact ? 'h-7 px-2.5 text-xs rounded-lg' : 'h-10 px-3 text-sm rounded-[var(--radius-md)]'} border
          bg-[var(--color-surface)] text-[var(--color-text-primary)]
          placeholder:text-[var(--color-text-tertiary)]
          transition-colors duration-150
          ${error
            ? 'border-[var(--color-error)] focus:shadow-[var(--shadow-error-ring)]'
            : 'border-[var(--color-border)] focus:border-[var(--color-border-focus)] focus:shadow-[var(--shadow-focus-ring)]'
          }
          outline-none
          ${className}
        `}
        {...props}
      />
      {error && <p className="text-xs text-[var(--color-error)]">{error}</p>}
    </div>
  )
}
