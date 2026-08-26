// SPDX-License-Identifier: MIT

function firstKey(input: unknown, keys: readonly string[]): unknown {
  if (!input || typeof input !== 'object') return undefined
  const obj = input as Record<string, unknown>
  for (const k of keys) {
    if (obj[k] !== undefined && obj[k] !== null && obj[k] !== '') return obj[k]
  }
  return undefined
}

function readString(input: unknown, keys: readonly string[]): string | undefined {
  const value = firstKey(input, keys)
  return typeof value === 'string' ? value : undefined
}

function readNumber(input: unknown, keys: readonly string[]): number | undefined {
  const value = firstKey(input, keys)
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value === 'string') {
    const n = Number(value)
    if (Number.isFinite(n)) return n
  }
  return undefined
}

export function basename(path: string | undefined): string {
  if (!path) return ''
  const trimmed = path.replace(/[\\/]+$/, '')
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  return idx >= 0 ? trimmed.slice(idx + 1) : trimmed
}

export function splitPathForDisplay(
  path: string | undefined,
): { dir: string; tail: string; separator: string } {
  if (!path) return { dir: '', tail: '', separator: '/' }
  let p = path
  const extendedMatch = /^\\\\\?\\(.+)$/.exec(p)
  if (extendedMatch && extendedMatch[1]) {
    p = extendedMatch[1]
  }
  const trimmed = p.replace(/[\\/]+$/, '')
  const separator = trimmed.includes('\\') && !trimmed.includes('/') ? '\\' : '/'
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  if (idx < 0) return { dir: '', tail: trimmed, separator }
  return {
    dir: trimmed.slice(0, idx),
    tail: trimmed.slice(idx + 1),
    separator,
  }
}

export function isWorkspaceRootPath(raw: string | undefined): boolean {
  const t = raw?.trim() ?? ''
  return t === '' || t === '.' || t === './' || t === '.\\'
}

export function meaningfulListPath(raw: string | undefined): string {
  const t = raw?.trim() ?? ''
  if (!t || t === '.' || t === './' || t === '.\\') return ''
  return t
}

export function isGlobPattern(s: string | undefined): boolean {
  if (!s) return false
  return /[*?[\]{}]/.test(s)
}

export function extractPath(input: unknown): string {
  return (
    readString(input, [
      'path',
      'file_path',
      'filepath',
      'filename',
      'notebook_path',
      'target',
      'target_path',
      'glob_pattern',
      'directory',
    ]) ?? ''
  )
}

export type EditTarget =
  | { kind: 'path'; full: string; dir: string; tail: string; separator: string; isWorkspaceRoot: boolean }
  | { kind: 'glob'; pattern: string }
  | { kind: 'multi'; first: string; count: number; paths: string[] }
  | { kind: 'unknown' }

function collectMultiEditPaths(input: unknown): string[] {
  if (!input || typeof input !== 'object') return []
  const edits = (input as Record<string, unknown>).edits
  if (!Array.isArray(edits)) return []
  const paths: string[] = []
  for (const entry of edits) {
    if (entry && typeof entry === 'object') {
      const p = (entry as Record<string, unknown>).path
      if (typeof p === 'string' && p.trim()) paths.push(p)
    }
  }
  return paths
}

export function resolveEditTarget(
  input: unknown,
  toolName: string,
): EditTarget {
  const multi = collectMultiEditPaths(input)
  if (multi.length > 0) {
    return {
      kind: 'multi',
      first: multi[0] ?? '',
      count: multi.length,
      paths: multi,
    }
  }

  const rawPattern =
    (input && typeof input === 'object'
      ? (input as Record<string, unknown>).pattern
      : undefined)
  if (typeof rawPattern === 'string' && rawPattern.trim()) {
    if (toolName === 'glob_edit' || isGlobPattern(rawPattern)) {
      return { kind: 'glob', pattern: rawPattern }
    }
  }

  const globKey =
    (input && typeof input === 'object'
      ? (input as Record<string, unknown>).glob_pattern
      : undefined)
  if (typeof globKey === 'string' && globKey.trim()) {
    return { kind: 'glob', pattern: globKey }
  }

  const direct = extractPath(input)
  if (direct) {
    if (isGlobPattern(direct)) {
      return { kind: 'glob', pattern: direct }
    }
    const { dir, tail, separator } = splitPathForDisplay(direct)
    return {
      kind: 'path',
      full: direct,
      dir,
      tail,
      separator,
      isWorkspaceRoot: isWorkspaceRootPath(direct),
    }
  }

  return { kind: 'unknown' }
}

export function extractQuery(input: unknown): string {
  return (
    readString(input, ['query', 'q', 'pattern', 'search', 'keyword', 'prompt']) ?? ''
  )
}

export function extractUrl(input: unknown): string {
  return readString(input, ['url', 'href', 'link']) ?? ''
}

export function extractAction(input: unknown): string {
  return readString(input, ['action', 'op', 'method']) ?? ''
}

export function extractCommand(input: unknown): string {
  return readString(input, ['command', 'cmd', 'script']) ?? ''
}

export function firstWord(command: string): string {
  const trimmed = command.trim()
  if (!trimmed) return ''

  const afterAnd = trimmed.split(/&&|\|\|/).pop()?.trim() ?? trimmed
  const tokens = afterAnd.split(/\s+/)
  return tokens[0] ?? ''
}

export type FileRange = { offset?: number; limit?: number }

export function extractRange(input: unknown): FileRange {
  return {
    offset: readNumber(input, ['offset', 'start', 'startLine', 'start_line']),
    limit: readNumber(input, ['limit', 'count', 'lines', 'maxLines']),
  }
}

export function urlHost(url: string): string {
  if (!url) return ''
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return ''
  }
}

export function firstLine(text: string): string {
  if (!text) return ''
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    if (line) return line
  }
  return ''
}

export function extractTextContent(content: unknown): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map((chunk) => {
        if (typeof chunk === 'string') return chunk
        if (chunk && typeof chunk === 'object' && 'text' in chunk) {
          const t = (chunk as { text?: unknown }).text
          return typeof t === 'string' ? t : ''
        }
        return ''
      })
      .filter(Boolean)
      .join('\n')
  }
  if (content && typeof content === 'object') {
    try {
      return JSON.stringify(content, null, 2)
    } catch {
      return String(content)
    }
  }
  if (content === null || content === undefined) return ''
  return String(content)
}

export function truncate(s: string, max: number): string {
  if (!s) return ''
  if (s.length <= max) return s
  return `${s.slice(0, Math.max(0, max - 1))}…`
}

export type SearchHit = {

  file: string

  line?: number

  preview: string
}

export function parseSearchHits(text: string, max = 50): SearchHit[] {
  if (!text) return []
  const hits: SearchHit[] = []
  for (const raw of text.split(/\r?\n/)) {
    if (hits.length >= max) break
    const line = raw.replace(/\u001b\[[0-9;]*m/g, '').replace(/\s+$/, '')
    if (!line) continue
    if (/^---+$|^==+$/.test(line)) continue

    const m1 = line.match(/^([^:\r\n]+?):(\d+):(.*)$/)
    if (m1) {
      hits.push({
        file: m1[1] ?? '',
        line: Number(m1[2]),
        preview: (m1[3] ?? '').trim(),
      })
      continue
    }

    const m2 = line.match(/^([^\s:][^\r\n]*?)-(\d+)-(.*)$/)
    if (m2) {
      hits.push({
        file: m2[1] ?? '',
        line: Number(m2[2]),
        preview: (m2[3] ?? '').trim(),
      })
      continue
    }

    const m3 = line.match(/^([^:\s][^:\r\n]*\.[A-Za-z0-9_.+-]{1,8}):(.*)$/)
    if (m3) {
      hits.push({ file: m3[1] ?? '', preview: (m3[2] ?? '').trim() })
      continue
    }

    hits.push({ file: '', preview: line.trim() })
  }
  return hits
}

export function lineCountLabel(count: number | undefined): string {
  if (!count || count <= 0) return ''
  return `${count.toLocaleString()} ${count === 1 ? 'line' : 'lines'}`
}

export type WebSearchHit = {
  title: string
  url: string
  snippet?: string
  host: string
  source?: string
  publishedAt?: string
  engine?: string
  index?: number
}

export type WebSearchSummary = {
  query: string
  provider?: string
  engine?: string
  successfulEngines?: string[]
  hits: WebSearchHit[]
  raw: string
  looksLikeError: boolean
  errorMessage?: string
  fallbackHeader?: string
}

const WEB_SEARCH_ENVELOPE_RE =
  /===WEB_SEARCH_JSON_BEGIN===\s*([\s\S]*?)\s*===WEB_SEARCH_JSON_END===/

function parseWebSearchEnvelope(text: string): WebSearchSummary | null {
  if (!text) return null
  const match = text.match(WEB_SEARCH_ENVELOPE_RE)
  if (!match || !match[1]) return null
  let payload: unknown
  try {
    payload = JSON.parse(match[1].trim())
  } catch {
    return null
  }
  if (!payload || typeof payload !== 'object') return null
  const obj = payload as Record<string, unknown>
  const query = typeof obj.query === 'string' ? obj.query : ''
  const successfulEngines = Array.isArray(obj.successful_engines)
    ? (obj.successful_engines.filter((s) => typeof s === 'string') as string[])
    : undefined
  const rawHits = Array.isArray(obj.hits) ? obj.hits : []
  const hits: WebSearchHit[] = []
  for (const h of rawHits) {
    if (!h || typeof h !== 'object') continue
    const hobj = h as Record<string, unknown>
    const title = typeof hobj.title === 'string' ? hobj.title.trim() : ''
    const url = typeof hobj.url === 'string' ? hobj.url.trim() : ''
    if (!title || !url) continue
    const description =
      typeof hobj.description === 'string' ? hobj.description.trim() : ''
    const sourceRaw = typeof hobj.source === 'string' ? hobj.source.trim() : ''
    const engineRaw = typeof hobj.engine === 'string' ? hobj.engine.trim() : ''
    const publishedRaw =
      typeof hobj.publishedAt === 'string' ? hobj.publishedAt.trim() : ''
    const hostRaw = typeof hobj.host === 'string' ? hobj.host.trim() : ''
    const indexRaw = typeof hobj.index === 'number' ? hobj.index : undefined
    hits.push({
      title,
      url,
      snippet: description || undefined,
      host: hostRaw || safeHost(url),
      source: sourceRaw || undefined,
      engine: engineRaw || undefined,
      publishedAt: publishedRaw || undefined,
      index: indexRaw,
    })
  }
  if (hits.length === 0 && !query) return null
  return {
    query,
    successfulEngines,
    hits,
    raw: text,
    looksLikeError: false,
  }
}

const WEB_SEARCH_ERROR_PREFIXES = [
  'Error:',
  'error:',
  'Error executing ',
  'Unknown tool: ',
  '[Tool error]',
  '[Refused]',
  'Tool failed:',
  'Blocked by guardrails:',
]

const WEB_SEARCH_ERROR_NEEDLES = [
  'error sending request for url',
  'failed to send request',
  'connection refused',
  'connection reset',
  'operation timed out',
  'dns error',
  'tls handshake',
  'all web search providers failed',
  'baidu blocked the request',
]

function detectWebSearchError(raw: string): { isError: boolean; message?: string } {
  const trimmed = raw.trimStart()
  if (!trimmed) return { isError: false }
  for (const prefix of WEB_SEARCH_ERROR_PREFIXES) {
    if (trimmed.startsWith(prefix)) {
      const firstLine = trimmed.split(/\r?\n/, 1)[0] ?? trimmed
      return { isError: true, message: firstLine.trim() }
    }
  }
  const head = trimmed.slice(0, 1024).toLowerCase()
  for (const needle of WEB_SEARCH_ERROR_NEEDLES) {
    if (head.includes(needle)) {
      const lineWithNeedle =
        trimmed
          .split(/\r?\n/)
          .find((line) => line.toLowerCase().includes(needle)) ?? trimmed
      return { isError: true, message: lineWithNeedle.trim() }
    }
  }
  return { isError: false }
}

export function safeHost(url: string): string {
  if (!url) return ''
  try {
    const u = new URL(url)
    return u.hostname.replace(/^www\./i, '')
  } catch {
    const m = url.match(/^[a-z]+:\/\/([^/]+)/i)
    if (m && m[1]) return m[1].replace(/^www\./i, '')
    const slashSplit = url.split('/').filter(Boolean)
    return (slashSplit[0] ?? url).replace(/^www\./i, '')
  }
}

export function parseWebSearchResults(text: string): WebSearchSummary {
  if (text) {
    const envelope = parseWebSearchEnvelope(text)
    if (envelope) return envelope
  }

  const summary: WebSearchSummary = {
    query: '',
    hits: [],
    raw: text || '',
    looksLikeError: false,
  }
  if (!text) return summary

  const errorDetection = detectWebSearchError(text)
  if (errorDetection.isError) {
    summary.looksLikeError = true
    summary.errorMessage = errorDetection.message
  }

  const lines = text.split(/\r?\n/)
  let cursor = 0
  while (cursor < lines.length && !(lines[cursor] ?? '').trim()) cursor += 1

  if (cursor < lines.length) {
    const head = (lines[cursor] ?? '').trim()
    const sourcesMatch = head.match(
      /^Sources:\s*\d+\s*engine\(s\)\s*returned\s*\d+\s*aggregated\s*result\(s\)\s*[—-]\s*(.+)$/i,
    )
    if (sourcesMatch && sourcesMatch[1]) {
      summary.successfulEngines = sourcesMatch[1]
        .split(/,\s*/)
        .map((s) => s.trim())
        .filter(Boolean)
      cursor += 1
      while (cursor < lines.length && !(lines[cursor] ?? '').trim()) cursor += 1
    }
  }

  if (cursor < lines.length) {
    const head = (lines[cursor] ?? '').trim()
    const fallbackMatch = head.match(/^\[Fallback\]\s*(.+)$/i)
    if (fallbackMatch && fallbackMatch[1]) {
      summary.fallbackHeader = fallbackMatch[1].trim()
      cursor += 1
      while (cursor < lines.length && !(lines[cursor] ?? '').trim()) cursor += 1
    }
  }
  if (cursor < lines.length) {
    const head = (lines[cursor] ?? '').trim()
    const headMatch = head.match(
      /^(?:#\s*Web\s+Search\s+Results\s+for|Search\s+results\s+for):\s*(.+?)(?:\s+\(via\s+([^)]+)\))?\.?$/i,
    )
    if (headMatch) {
      summary.query = (headMatch[1] ?? '').trim()
      if (headMatch[2]) summary.provider = headMatch[2].trim()
      cursor += 1
    } else if (/^No results found for:/i.test(head)) {
      const m = head.match(/^No results found for:\s*(.+)$/i)
      if (m && m[1]) summary.query = m[1].trim()
      return summary
    }
  }

  while (cursor < lines.length) {
    const probe = (lines[cursor] ?? '').trim()
    if (!probe) {
      cursor += 1
      continue
    }
    const foundMatch = probe.match(/^Found\s+\d+\s+result/i)
    if (foundMatch) {
      cursor += 1
      continue
    }
    const engineMatch = probe.match(/^Engine:\s*(.+)$/i)
    if (engineMatch && engineMatch[1]) {
      summary.engine = engineMatch[1].trim().toLowerCase()
      cursor += 1
      continue
    }
    break
  }

  let i = cursor
  let indexCounter = 0
  while (i < lines.length) {
    const numberMatch =
      lines[i]?.match(/^\s*(\d+)\.\s+(.+?)\s*$/) ??
      lines[i]?.match(/^##\s*(\d+)\.\s+(.+?)\s*$/)
    if (!numberMatch) {
      i += 1
      continue
    }
    indexCounter += 1
    const title = (numberMatch[2] ?? '').trim()
    let url = ''
    let snippet: string | undefined
    let engine: string | undefined
    let source: string | undefined
    let publishedAt: string | undefined

    let j = i + 1
    while (j < lines.length) {
      const next = lines[j]
      if (!next || !next.trim()) {
        j += 1
        continue
      }
      if (/^\s*\d+\.\s+/.test(next) || /^##\s*\d+\.\s+/.test(next)) break
      const indented = /^\s+\S/.test(next) || /^[A-Z]/.test(next)
      if (!indented && !url) break
      const stripped = next.trim()

      const urlMatch = stripped.match(/^URL:\s*(.+)$/i)
      if (!url && urlMatch && urlMatch[1]) {
        url = urlMatch[1].trim()
        j += 1
        continue
      }
      const engineFieldMatch = stripped.match(/^Engine:\s*(.+)$/i)
      if (engineFieldMatch && engineFieldMatch[1]) {
        engine = engineFieldMatch[1].trim()
        j += 1
        continue
      }
      const sourceFieldMatch = stripped.match(/^Source:\s*(.+)$/i)
      if (sourceFieldMatch && sourceFieldMatch[1]) {
        source = sourceFieldMatch[1].trim()
        j += 1
        continue
      }
      const publishedFieldMatch = stripped.match(/^Published:\s*(.+)$/i)
      if (publishedFieldMatch && publishedFieldMatch[1]) {
        publishedAt = publishedFieldMatch[1].trim()
        j += 1
        continue
      }
      if (!url && /^[a-z][a-z0-9+\-.]*:\/\//i.test(stripped)) {
        url = stripped
        j += 1
        continue
      }
      if (url && !snippet) {
        snippet = stripped
      } else if (url && snippet) {
        snippet = `${snippet} ${stripped}`.trim()
      }
      j += 1
    }

    summary.hits.push({
      title,
      url,
      snippet,
      host: safeHost(url),
      source,
      publishedAt,
      engine,
      index: indexCounter,
    })
    i = j
  }

  return summary
}

export const WEB_SEARCH_TOOL_NAMES: ReadonlySet<string> = new Set([
  'web_search',
  'web_search_tool',
  'WebSearch',
  'tavily_search',
  'exa_search',
  'multi_search',
  'github_search',
  'youtube_search',
  'reddit_search',
  'image_search',
  'discord_search',
])

export function isWebSearchTool(name: string | undefined | null): boolean {
  return !!name && WEB_SEARCH_TOOL_NAMES.has(name)
}
