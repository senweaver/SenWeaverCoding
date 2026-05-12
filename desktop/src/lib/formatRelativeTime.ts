// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding

export function formatRelativeTime(input: string | number | undefined, now?: number): string {
  if (!input) return ''
  const ts =
    typeof input === 'number'
      ? input
      : Date.parse(input)
  if (!Number.isFinite(ts)) return ''
  const reference = now ?? Date.now()
  const diff = reference - ts
  if (diff < 0) return 'now'
  const seconds = Math.floor(diff / 1000)
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  const weeks = Math.floor(days / 7)
  if (weeks < 5) return `${weeks}w`
  const months = Math.floor(days / 30)
  if (months < 12) return `${months}mo`
  const years = Math.floor(days / 365)
  return `${years}y`
}

export function formatAbsoluteTime(input: string | number | undefined): string {
  if (!input) return ''
  const ts = typeof input === 'number' ? input : Date.parse(input)
  if (!Number.isFinite(ts)) return ''
  try {
    return new Date(ts).toLocaleString()
  } catch {
    return ''
  }
}
