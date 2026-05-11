import { useMemo, useState } from 'react'
import type { ToolViewProps } from './ToolViewProps'
import { useTranslation } from '../../../i18n'
import {
  extractQuery,
  extractTextContent,
  parseWebSearchResults,
  safeHost,
  truncate,
  type WebSearchHit,
  type WebSearchSummary,
} from '../../../utils/toolFormatters'
import {
  engineIconFor,
  engineIconForHost,
  engineLabelFor,
  isEngineId,
  type EngineId,
} from './engineIcons'

type AvatarColor = { bg: string; fg: string }

const AVATAR_PALETTE: readonly AvatarColor[] = [
  { bg: '#2563eb', fg: '#ffffff' },
  { bg: '#dc2626', fg: '#ffffff' },
  { bg: '#f97316', fg: '#ffffff' },
  { bg: '#0ea5e9', fg: '#ffffff' },
  { bg: '#16a34a', fg: '#ffffff' },
  { bg: '#a855f7', fg: '#ffffff' },
  { bg: '#ea580c', fg: '#ffffff' },
  { bg: '#0891b2', fg: '#ffffff' },
] as const

const AVATAR_FALLBACK: AvatarColor = { bg: '#64748b', fg: '#ffffff' }

const KNOWN_ENGINE_IDS: ReadonlySet<string> = new Set([
  'duckduckgo',
  'brave',
  'bing',
  'baidu',
  'csdn',
  'juejin',
  'zhihu',
  'jina',
  'weixin',
  'wechat',
  'github',
  'arxiv',
  'semanticscholar',
  'dblp',
  'pubmed',
  'googlescholar',
  'searxng',
  'sogou',
  'people',
  'xinhuanet',
])

function engineIdFromName(raw: string | null | undefined): EngineId | null {
  if (!raw) return null
  const norm = raw.toLowerCase().replace(/[\s_-]/g, '')
  if (norm.includes('duckduckgo') || norm === 'ddg') return 'duckduckgo'
  if (norm.includes('brave')) return 'brave'
  if (norm.includes('bing')) return 'bing'
  if (norm.includes('baidu')) return 'baidu'
  if (norm.includes('searxng') || norm.includes('searx')) return 'searxng'
  if (norm.includes('csdn')) return 'csdn'
  if (norm.includes('jina')) return 'jina'
  if (norm.includes('github')) return 'github'
  if (norm.includes('zhihu')) return 'zhihu'
  if (norm.includes('juejin')) return 'juejin'
  if (norm.includes('weixin') || norm.includes('wechat')) return 'weixin'
  if (norm.includes('arxiv')) return 'arxiv'
  if (norm.includes('semanticscholar')) return 'semanticscholar'
  if (norm.includes('dblp')) return 'dblp'
  if (norm.includes('pubmed')) return 'pubmed'
  if (norm.includes('googlescholar') || norm.includes('scholar')) return 'googlescholar'
  if (norm.includes('sogou')) return 'sogou'
  if (KNOWN_ENGINE_IDS.has(norm)) return norm as EngineId
  return null
}

function collectEngineIds(summary: WebSearchSummary): EngineId[] {
  const seen = new Set<EngineId>()
  const order: EngineId[] = []
  const push = (id: EngineId | null) => {
    if (!id || seen.has(id)) return
    seen.add(id)
    order.push(id)
  }
  push(engineIdFromName(summary.engine))
  push(engineIdFromName(summary.provider))
  if (summary.fallbackHeader) {
    const m = summary.fallbackHeader.match(/Primary\s+([\w-]+)\s+failed[^;]*;\s*results from\s+([\w-]+)/i)
    if (m) {
      push(engineIdFromName(m[1]))
      push(engineIdFromName(m[2]))
    } else {
      const tokens = summary.fallbackHeader
        .split(/[^\w-]+/)
        .map((t) => t.trim())
        .filter(Boolean)
      for (const t of tokens) push(engineIdFromName(t))
    }
  }
  return order
}

function hashHost(host: string): number {
  let h = 0
  for (let i = 0; i < host.length; i += 1) {
    h = (h * 31 + host.charCodeAt(i)) | 0
  }
  return Math.abs(h)
}

function avatarLetter(host: string): string {
  if (!host) return '·'
  const c = host.charAt(0)
  return c ? c.toUpperCase() : '·'
}

function avatarColors(host: string): AvatarColor {
  if (!host || AVATAR_PALETTE.length === 0) return AVATAR_FALLBACK
  return AVATAR_PALETTE[hashHost(host) % AVATAR_PALETTE.length] ?? AVATAR_FALLBACK
}

function HostAvatar({ host, size = 14 }: { host: string; size?: number }) {
  const colors = avatarColors(host)
  return (
    <span
      className="inline-flex shrink-0 items-center justify-center rounded-full font-semibold"
      style={{
        width: size,
        height: size,
        backgroundColor: colors.bg,
        color: colors.fg,
        fontSize: Math.max(8, Math.floor(size * 0.6)),
        lineHeight: 1,
      }}
      title={host}
    >
      {avatarLetter(host)}
    </span>
  )
}

function HostFavicon({
  host,
  size = 16,
  rounded = 'full',
}: {
  host: string
  size?: number
  rounded?: 'full' | 'sm'
}) {
  const localIcon = useMemo(() => engineIconForHost(host), [host])
  const [errored, setErrored] = useState(false)
  const radiusClass = rounded === 'full' ? 'rounded-full' : 'rounded-sm'

  if (!host || !localIcon || errored) {
    return <HostAvatar host={host} size={size} />
  }

  return (
    <img
      src={localIcon}
      alt={host}
      title={host}
      width={size}
      height={size}
      loading="lazy"
      decoding="async"
      onError={() => setErrored(true)}
      className={`${radiusClass} shrink-0 bg-[var(--color-surface-container-high)]/40 object-contain`}
      style={{ width: size, height: size }}
    />
  )
}

function EngineIcon({
  id,
  size = 16,
  rounded = 'sm',
}: {
  id: EngineId
  size?: number
  rounded?: 'full' | 'sm'
}) {
  const src = engineIconFor(id)
  const label = engineLabelFor(id)
  const radiusClass = rounded === 'full' ? 'rounded-full' : 'rounded-sm'
  const [errored, setErrored] = useState(false)
  if (!src || errored) {
    return <HostAvatar host={label} size={size} />
  }
  return (
    <img
      src={src}
      alt={label}
      title={label}
      width={size}
      height={size}
      loading="lazy"
      decoding="async"
      onError={() => setErrored(true)}
      className={`${radiusClass} shrink-0 bg-[var(--color-surface-container-high)]/40 object-contain`}
      style={{ width: size, height: size }}
    />
  )
}

function uniqueHosts(hits: WebSearchHit[], max: number): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const hit of hits) {
    if (!hit.host) continue
    if (seen.has(hit.host)) continue
    seen.add(hit.host)
    out.push(hit.host)
    if (out.length >= max) break
  }
  return out
}

function readSummary(props: ToolViewProps): WebSearchSummary {
  const text = props.result ? extractTextContent(props.result.content) : ''
  return parseWebSearchResults(text)
}

function fallbackQuery(props: ToolViewProps): string {
  const q = extractQuery(props.input)
  return typeof q === 'string' ? q : ''
}

export function WebSearchHeader(props: ToolViewProps) {
  const { isStreaming, result } = props
  const t = useTranslation()
  const summary = useMemo(() => readSummary(props), [props])
  const query = summary.query || fallbackQuery(props)
  const hostChips = uniqueHosts(summary.hits, 5)
  const isError = result?.isError === true || summary.looksLikeError
  const dotClass = isError
    ? 'bg-[var(--color-error)]'
    : isStreaming
      ? 'bg-[var(--color-warning)] animate-pulse'
      : 'bg-[var(--color-success)]'

  const label = isStreaming
    ? t('tool.web.searchInProgress')
    : isError
      ? t('tool.web.searchFailed')
      : t('tool.web.searchDone')

  return (
    <span className="flex min-w-0 flex-1 items-center gap-2">
      <span className={`inline-block h-2 w-2 shrink-0 rounded-full ${dotClass}`} />
      <span
        className={`shrink-0 text-[12px] font-medium ${
          isError ? 'text-[var(--color-error)]' : 'text-[var(--color-text-primary)]'
        }`}
      >
        {label}
      </span>
      {query && (
        <span
          className="min-w-0 flex-1 truncate rounded bg-[var(--color-surface-container-high)]/60 px-1.5 py-0.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]"
          title={query}
        >
          {truncate(query, 80)}
        </span>
      )}
      {hostChips.length > 0 && (
        <span className="ml-auto flex shrink-0 items-center -space-x-1">
          {hostChips.map((host) => (
            <span
              key={host}
              className="rounded-full ring-2 ring-[var(--color-surface-container-lowest)]"
            >
              <HostFavicon host={host} size={16} rounded="full" />
            </span>
          ))}
        </span>
      )}
      {summary.hits.length > 0 && !isError && (
        <span className="shrink-0 text-[11px] text-[var(--color-text-tertiary)]">
          {t('tool.web.readPages', { count: summary.hits.length })}
        </span>
      )}
    </span>
  )
}

export function WebSearchDetail(props: ToolViewProps) {
  const t = useTranslation()
  const summary = useMemo(() => readSummary(props), [props])
  const engineIds = useMemo(() => collectEngineIds(summary), [summary])
  const isStreaming = props.isStreaming === true
  const hasError = props.result?.isError === true || summary.looksLikeError

  if (isStreaming && summary.hits.length === 0) {
    return (
      <div className="text-[11px] text-[var(--color-text-tertiary)]">
        {t('tool.web.searchInProgress')}
      </div>
    )
  }

  if (hasError) {
    return (
      <div className="space-y-1.5">
        <div className="text-[11px] font-semibold text-[var(--color-error)]">
          {t('tool.web.searchFailed')}
        </div>
        {summary.errorMessage && (
          <div className="rounded border border-[var(--color-error)]/30 bg-[var(--color-error)]/10 px-2 py-1.5 text-[11px] text-[var(--color-error)]">
            {summary.errorMessage}
          </div>
        )}
        {summary.raw && summary.raw !== summary.errorMessage && (
          <pre className="max-h-48 overflow-auto rounded border border-[var(--color-border)]/60 bg-[var(--color-surface-container-lowest)] p-2 text-[11px] text-[var(--color-text-secondary)] whitespace-pre-wrap">
            {summary.raw}
          </pre>
        )}
      </div>
    )
  }

  if (summary.hits.length === 0) {
    return (
      <div className="space-y-1.5">
        <div className="text-[11px] text-[var(--color-text-tertiary)]">
          {t('tool.web.noResults')}
        </div>
        {summary.raw && (
          <pre className="max-h-32 overflow-auto rounded border border-[var(--color-border)]/60 bg-[var(--color-surface-container-lowest)] p-2 text-[11px] text-[var(--color-text-secondary)] whitespace-pre-wrap">
            {summary.raw}
          </pre>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-3">
      {(summary.fallbackHeader || engineIds.length > 0) && (
        <div className="flex min-w-0 items-center gap-2 text-[11px]">
          {engineIds.length > 0 && (
            <span
              className="flex shrink-0 items-center -space-x-1"
              aria-label={t('tool.web.enginesUsedLabel')}
              title={engineIds.map((id) => engineLabelFor(id)).join(' · ')}
            >
              {engineIds.map((id) => (
                <span
                  key={id}
                  className="rounded-full ring-2 ring-[var(--color-surface-container-lowest)]"
                >
                  <EngineIcon id={id} size={14} rounded="full" />
                </span>
              ))}
            </span>
          )}
          {summary.fallbackHeader && (
            <span className="min-w-0 truncate text-[var(--color-text-tertiary)]">
              {summary.fallbackHeader}
            </span>
          )}
          {!summary.fallbackHeader &&
            summary.provider &&
            !isEngineId(summary.engine) && (
              <span className="text-[var(--color-text-tertiary)]">
                {t('tool.web.providerLabel')}
                <span className="ml-1 font-medium text-[var(--color-text-secondary)]">
                  {summary.provider}
                </span>
              </span>
            )}
        </div>
      )}
      <ol className="space-y-3.5">
        {summary.hits.map((hit, index) => (
          <li
            key={`${hit.url || 'noref'}-${index}`}
            className="group min-w-0 rounded-md px-1 py-0.5 hover:bg-[var(--color-surface-container-high)]/40"
          >
            <div className="flex min-w-0 items-start gap-3">
              <span className="mt-1 shrink-0">
                <HostFavicon
                  host={hit.host || safeHost(hit.url)}
                  size={18}
                  rounded="sm"
                />
              </span>
              <div className="min-w-0 flex-1 space-y-1">
                {hit.url ? (
                  <a
                    href={hit.url}
                    target="_blank"
                    rel="noreferrer noopener"
                    className="block truncate text-[14px] font-medium text-[var(--color-text-primary)] group-hover:text-[var(--color-text-accent)] hover:underline"
                    title={hit.title}
                  >
                    {hit.title || hit.url}
                  </a>
                ) : (
                  <span className="block truncate text-[14px] font-medium text-[var(--color-text-primary)]">
                    {hit.title || '(untitled)'}
                  </span>
                )}
                {hit.url && (
                  <div
                    className="truncate text-[11.5px] text-[var(--color-text-tertiary)]"
                    title={hit.url}
                  >
                    {hit.url}
                  </div>
                )}
                {hit.snippet && (
                  <p
                    className="text-[12.5px] leading-[1.55] text-[var(--color-text-secondary)] line-clamp-3"
                    title={hit.snippet}
                  >
                    {hit.snippet}
                  </p>
                )}
              </div>
            </div>
          </li>
        ))}
      </ol>
    </div>
  )
}
