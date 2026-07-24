// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'

import { getAuthToken } from '../../api/client'

type Props = {
  src: string
  alt: string
  reloadKey?: string | number
  onOpen?: () => void
  onError?: () => void
  pendingLabel: string
  failedLabel: string
}

export function LanImage({
  src,
  alt,
  reloadKey,
  onOpen,
  onError,
  pendingLabel,
  failedLabel,
}: Props) {
  const [objectUrl, setObjectUrl] = useState<string | null>(null)
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading')

  useEffect(() => {
    let cancelled = false
    let createdUrl: string | null = null
    setStatus('loading')
    setObjectUrl(null)
    void (async () => {
      try {
        const token = getAuthToken()
        const res = await fetch(src, {
          cache: 'no-store',
          headers: token ? { 'X-Sen-Gateway-Token': token } : undefined,
        })
        if (!res.ok) throw new Error(`status ${res.status}`)
        const blob = await res.blob()
        if (cancelled) return
        createdUrl = URL.createObjectURL(blob)
        setObjectUrl(createdUrl)
        setStatus('ready')
      } catch {
        if (!cancelled) {
          setStatus('error')
          onError?.()
        }
      }
    })()
    return () => {
      cancelled = true
      if (createdUrl) URL.revokeObjectURL(createdUrl)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [src, reloadKey])

  if (status === 'error') {
    return (
      <span className="block rounded-lg bg-[var(--color-surface-hover)] px-2 py-3 text-center text-[11px] text-[var(--color-text-tertiary)]">
        {failedLabel}
      </span>
    )
  }

  if (status === 'loading' || !objectUrl) {
    return (
      <span className="flex h-24 w-40 items-center justify-center rounded-lg bg-[var(--color-surface-hover)] text-[11px] text-[var(--color-text-tertiary)]">
        <span className="material-symbols-outlined animate-spin text-[16px]">progress_activity</span>
        <span className="ml-1">{pendingLabel}</span>
      </span>
    )
  }

  return (
    <img
      src={objectUrl}
      alt={alt}
      onClick={onOpen}
      className="max-h-64 max-w-full cursor-zoom-in rounded-lg object-contain"
    />
  )
}
