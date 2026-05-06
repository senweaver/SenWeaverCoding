// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Helpers for resolving the user-facing title of a chat session.
//
// Sessions created before the auto-derive logic landed often carry
// generic placeholder names like `Session 21:50` or `Untitled session`.
// This module centralises the detection of those placeholders so that
// the sidebar, tab bar, and tab restoration logic all show a single
// localised "new conversation" label until a real summary is set.

const LEGACY_PLACEHOLDER_LITERALS = new Set<string>([
  'Untitled session',
  'Untitled',
  'New Session',
  'New conversation',
  '新对话',
  '未命名会话',
])

const LEGACY_TIMESTAMP = /^Session \d{2}:\d{2}$/

export function isPlaceholderTitle(raw: string | null | undefined): boolean {
  const trimmed = (raw ?? '').trim()
  if (!trimmed) return true
  if (LEGACY_PLACEHOLDER_LITERALS.has(trimmed)) return true
  return LEGACY_TIMESTAMP.test(trimmed)
}

export function resolveSessionTitle(
  raw: string | null | undefined,
  fallback: string,
): string {
  const trimmed = (raw ?? '').trim()
  return isPlaceholderTitle(trimmed) ? fallback : trimmed
}
