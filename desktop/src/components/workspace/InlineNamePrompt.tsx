import { useEffect, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'

type Props = {
  initial?: string
  placeholder?: string
  onCancel: () => void
  onSubmit: (value: string) => void
}

export function InlineNamePrompt({ initial, placeholder, onCancel, onSubmit }: Props) {
  const t = useTranslation()
  const [value, setValue] = useState(initial ?? '')
  const ref = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    ref.current?.focus()
    ref.current?.select()
  }, [])

  return (
    <input
      ref={ref}
      type="text"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      placeholder={placeholder ?? t('files.namePlaceholder')}
      onBlur={() => onCancel()}
      onKeyDown={(event) => {
        if (event.key === 'Enter') {
          event.preventDefault()
          const trimmed = value.trim()
          if (trimmed.length > 0) onSubmit(trimmed)
          else onCancel()
        } else if (event.key === 'Escape') {
          event.preventDefault()
          onCancel()
        }
      }}
      className="w-full rounded border border-[var(--color-accent)] bg-[var(--color-surface)] px-1 py-0.5 text-xs text-[var(--color-text-primary)] outline-none"
    />
  )
}
