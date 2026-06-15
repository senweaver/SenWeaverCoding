// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export const MERMAID_LANGUAGE = 'mermaid'

export const MERMAID_PLAINTEXT_LANGUAGES = new Set(['', 'text', 'plaintext', 'plain'])

const MERMAID_DIAGRAM_TOKENS = new Set([
  'graph',
  'flowchart',
  'flowchart-elk',
  'sequencediagram',
  'classdiagram',
  'classdiagram-v2',
  'statediagram',
  'statediagram-v2',
  'erdiagram',
  'journey',
  'gantt',
  'pie',
  'gitgraph',
  'mindmap',
  'timeline',
  'requirement',
  'requirementdiagram',
  'quadrantchart',
  'xychart',
  'xychart-beta',
  'sankey',
  'sankey-beta',
  'block-beta',
  'packet',
  'packet-beta',
  'radar',
  'radar-beta',
  'treemap',
  'treemap-beta',
  'treeview-beta',
  'venn-beta',
  'wardley-beta',
  'eventmodeling',
  'ishikawa',
  'ishikawa-beta',
  'architecture',
  'architecture-beta',
  'kanban',
  'c4context',
  'c4container',
  'c4component',
  'c4dynamic',
  'c4deployment',
])

export function normalizeCodeLanguage(language: string | undefined): string {
  return language?.trim().split(/\s+/)[0]?.toLowerCase() ?? ''
}

function firstDiagramLine(code: string): string | undefined {
  const lines = code.split(/\r?\n/)
  let inDirective = false
  for (const rawLine of lines) {
    const line = rawLine.trim()
    if (!line) continue
    if (inDirective) {
      if (line.includes('}%%')) inDirective = false
      continue
    }
    if (line.startsWith('%%{')) {
      if (!line.includes('}%%')) inDirective = true
      continue
    }
    if (line.startsWith('%%')) continue
    return line
  }
  return undefined
}

export function looksLikeMermaid(code: string): boolean {
  const first = firstDiagramLine(code)
  if (!first) return false
  const token = first.split(/\s+/)[0]?.replace(/[:;]+$/, '').toLowerCase() ?? ''
  return MERMAID_DIAGRAM_TOKENS.has(token)
}

export function isMermaidBlock(language: string | undefined, code: string): boolean {
  const normalized = normalizeCodeLanguage(language)
  if (normalized === MERMAID_LANGUAGE) return true
  if (!MERMAID_PLAINTEXT_LANGUAGES.has(normalized)) return false
  return looksLikeMermaid(code)
}

type Fence = {
  char: string
  length: number
  language: string
}

function parseFenceOpen(line: string): Fence | null {
  const match = line.match(/^\s*(`{3,}|~{3,})(.*)$/)
  if (!match) return null
  const marker = match[1] ?? ''
  const info = (match[2] ?? '').trim()
  const char = marker[0] ?? '`'
  if (char === '`' && info.includes('`')) return null
  return { char, length: marker.length, language: info }
}

function isFenceClose(line: string, fence: Fence): boolean {
  const match = line.match(/^\s*(`{3,}|~{3,})\s*$/)
  if (!match) return false
  const marker = match[1] ?? ''
  return marker[0] === fence.char && marker.length >= fence.length
}

export function extractMermaidBlocks(markdown: string): string[] {
  const lines = markdown.split(/\r?\n/)
  const blocks: string[] = []
  let i = 0
  while (i < lines.length) {
    const fence = parseFenceOpen(lines[i] ?? '')
    if (fence) {
      const body: string[] = []
      i += 1
      while (i < lines.length && !isFenceClose(lines[i] ?? '', fence)) {
        body.push(lines[i] ?? '')
        i += 1
      }
      i += 1
      const code = body.join('\n')
      if (isMermaidBlock(fence.language, code)) {
        blocks.push(code)
      }
      continue
    }
    i += 1
  }
  return blocks
}
