// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState, type MouseEvent } from 'react'
import { useTranslation } from '../../i18n'
import { copyTextToClipboard } from '../chat/clipboard'

type Props = {
  text: string
  label?: string
  copiedLabel?: string
  displayLabel?: string
  displayCopiedLabel?: string
  className?: string

  onClick?: (event: MouseEvent<HTMLButtonElement>) => void
}

export function CopyButton({
  text,
  label,
  copiedLabel,
  displayLabel,
  displayCopiedLabel,
  className = '',
  onClick,
}: Props) {
  const t = useTranslation()
  const effectiveLabel = label ?? t('common.copy')
  const effectiveCopiedLabel = copiedLabel ?? t('common.copied')
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!copied) return
    const timer = window.setTimeout(() => setCopied(false), 1500)
    return () => window.clearTimeout(timer)
  }, [copied])

  const handleCopy = async (event: MouseEvent<HTMLButtonElement>) => {
    onClick?.(event)
    try {
      const ok = await copyTextToClipboard(text)
      if (!ok) {
        setCopied(false)
        return
      }
      setCopied(true)
    } catch {
      setCopied(false)
    }
  }

  const currentLabel = copied ? effectiveCopiedLabel : effectiveLabel
  const buttonText = copied
    ? (displayCopiedLabel ?? effectiveCopiedLabel)
    : (displayLabel ?? effectiveLabel)

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={className}
      aria-label={currentLabel}
      title={currentLabel}
    >
      {buttonText}
    </button>
  )
}
