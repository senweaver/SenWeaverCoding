// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const ENVELOPE_MARKER = '===DOC_CONVERT==='

export type DocConvertEnvelope = {
  path: string
  format: string
  bytes: number
  source: string | null
  font: string | null
}

export function parseDocConvertEnvelope(text: string): DocConvertEnvelope | null {
  const idx = text.lastIndexOf(ENVELOPE_MARKER)
  if (idx === -1) return null
  const raw = text.slice(idx + ENVELOPE_MARKER.length).trim()
  const newline = raw.indexOf('\n')
  const jsonLine = newline === -1 ? raw : raw.slice(0, newline)
  try {
    const parsed: unknown = JSON.parse(jsonLine)
    if (!parsed || typeof parsed !== 'object') return null
    const obj = parsed as Record<string, unknown>
    if (typeof obj.path !== 'string' || typeof obj.format !== 'string') return null
    return {
      path: obj.path,
      format: obj.format.toLowerCase(),
      bytes: typeof obj.bytes === 'number' ? obj.bytes : 0,
      source: typeof obj.source === 'string' && obj.source ? obj.source : null,
      font: typeof obj.font === 'string' && obj.font ? obj.font : null,
    }
  } catch {
    return null
  }
}

export function stripDocConvertEnvelope(text: string): string {
  const idx = text.lastIndexOf(ENVELOPE_MARKER)
  if (idx === -1) return text.trim()
  return text.slice(0, idx).trim()
}
