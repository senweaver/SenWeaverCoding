// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const LEGACY_PLACEHOLDER_LITERALS = new Set<string>([
  'untitled session',
  'untitled',
  'new session',
  'new conversation',
  'new agent',
  '新对话',
  '新智能体',
  '新建智能体',
  '未命名会话',
])

const LEGACY_TIMESTAMP = /^Session \d{2}:\d{2}$/

export function isPlaceholderTitle(raw: string | null | undefined): boolean {
  const trimmed = (raw ?? '').trim()
  if (!trimmed) return true
  if (LEGACY_PLACEHOLDER_LITERALS.has(trimmed.toLowerCase())) return true
  return LEGACY_TIMESTAMP.test(trimmed)
}

export function resolveSessionTitle(
  raw: string | null | undefined,
  fallback: string,
): string {
  const trimmed = (raw ?? '').trim()
  return isPlaceholderTitle(trimmed) ? fallback : trimmed
}
