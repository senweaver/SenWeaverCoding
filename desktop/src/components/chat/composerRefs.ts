// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type FileRefDragPayload = {
  relPath: string
  name: string
  isDir: boolean
}

export type RefSegment =
  | { type: 'text'; text: string }
  | { type: 'ref'; name: string; relPath: string }
  | { type: 'cred'; name: string }

export type RefKind = 'file' | 'folder' | 'session'

export const SESSION_REF_PREFIX = 'session:'

const CRED_NAME_CHARS = '[A-Za-z0-9_-]+'
const COMBINED_TOKEN_RE = new RegExp(
  `@\\[([^\\]\\n]*)\\]\\(([^)\\n]*)\\)|\\$\\{cred\\.(${CRED_NAME_CHARS})\\}`,
  'g',
)

export function makeCredToken(name: string): string {
  return `\${cred.${name}}`
}

export function makeSessionRefToken(name: string, sessionId: string): string {
  return makeRefToken(name, `${SESSION_REF_PREFIX}${sessionId}`)
}

export function isSessionRef(relPath: string): boolean {
  return relPath.trim().startsWith(SESSION_REF_PREFIX)
}

export function sessionIdFromRef(relPath: string): string {
  const trimmed = relPath.trim()
  return trimmed.startsWith(SESSION_REF_PREFIX)
    ? trimmed.slice(SESSION_REF_PREFIX.length)
    : ''
}

export function refKind(relPath: string): RefKind {
  if (isSessionRef(relPath)) return 'session'
  const base = lastSegment(relPath)
  return /\.[^.\\/]+$/.test(base) ? 'file' : 'folder'
}

export function makeRefToken(name: string, relPath: string): string {
  const safeName = name.replace(/[\]\n]/g, ' ').trim() || lastSegment(relPath) || 'ref'
  const safePath = relPath.replace(/[)\n]/g, ' ').trim()
  return `@[${safeName}](${safePath})`
}

export function parseRefSegments(value: string): RefSegment[] {
  const segments: RefSegment[] = []
  let lastIndex = 0
  COMBINED_TOKEN_RE.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = COMBINED_TOKEN_RE.exec(value)) !== null) {
    if (match.index > lastIndex) {
      segments.push({ type: 'text', text: value.slice(lastIndex, match.index) })
    }
    if (match[3] !== undefined) {
      segments.push({ type: 'cred', name: match[3] })
    } else {
      segments.push({ type: 'ref', name: match[1] ?? '', relPath: match[2] ?? '' })
    }
    lastIndex = match.index + match[0].length
  }
  if (lastIndex < value.length) {
    segments.push({ type: 'text', text: value.slice(lastIndex) })
  }
  return segments
}

export function hasRefTokens(value: string): boolean {
  COMBINED_TOKEN_RE.lastIndex = 0
  return COMBINED_TOKEN_RE.test(value)
}

export function refsToPlainText(value: string): string {
  if (!hasRefTokens(value)) return value
  return parseRefSegments(value)
    .map((segment) => {
      if (segment.type === 'text') return segment.text
      if (segment.type === 'cred') return segment.name
      return `@${segment.name || segment.relPath}`
    })
    .join('')
}

export function refIconName(relPath: string): string {
  if (isSessionRef(relPath)) return 'forum'
  const base = lastSegment(relPath)
  return /\.[^.\\/]+$/.test(base) ? 'description' : 'folder'
}

export function toRelativeRefPath(absPath: string, cwd: string, fallbackName: string): string {
  if (!cwd) return fallbackName
  const normPath = absPath.replace(/\\/g, '/')
  const normCwd = cwd.replace(/\\/g, '/').replace(/\/$/, '')
  if (normPath === normCwd) return fallbackName
  if (normPath.startsWith(`${normCwd}/`)) {
    return normPath.slice(normCwd.length + 1)
  }
  return normPath
}

function lastSegment(value: string): string {
  const parts = value.split(/[\\/]/)
  return parts[parts.length - 1] ?? value
}
