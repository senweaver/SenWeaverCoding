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

function faviconSources(host: string, size: number): string[] {
  if (!host) return []
  const sz = Math.max(16, Math.min(128, Math.round(size * 2)))
  const safe = encodeURIComponent(host)
  return [
    `https://www.google.com/s2/favicons?domain=${safe}&sz=${sz}`,
    `https://icons.duckduckgo.com/ip3/${safe}.ico`,
  ]
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
  const sources = useMemo(() => faviconSources(host, size), [host, size])
  const [index, setIndex] = useState(0)
  const radiusClass = rounded === 'full' ? 'rounded-full' : 'rounded-sm'

  if (!host) {
    return <HostAvatar host={host} size={size} />
  }

  if (index >= sources.length) {
    return <HostAvatar host={host} size={size} />
  }

  return (
    <img
      src={sources[index]}
      alt={host}
      title={host}
      width={size}
      height={size}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onError={() => setIndex((prev) => prev + 1)}
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
      {summary.fallbackHeader && (
        <div className="rounded border border-[var(--color-warning)]/30 bg-[var(--color-warning)]/10 px-2 py-1.5 text-[11px] text-[var(--color-text-secondary)]">
          {summary.fallbackHeader}
        </div>
      )}
      {summary.provider && (
        <div className="text-[11px] text-[var(--color-text-tertiary)]">
          {t('tool.web.providerLabel')}
          <span className="ml-1 font-medium text-[var(--color-text-secondary)]">
            {summary.provider}
          </span>
        </div>
      )}
      <ol className="space-y-2.5">
        {summary.hits.map((hit, index) => (
          <li key={`${hit.url || 'noref'}-${index}`} className="min-w-0">
            <div className="flex min-w-0 items-start gap-2">
              <span className="mt-0.5">
                <HostFavicon
                  host={hit.host || safeHost(hit.url)}
                  size={18}
                  rounded="sm"
                />
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline gap-2 min-w-0">
                  {hit.url ? (
                    <a
                      href={hit.url}
                      target="_blank"
                      rel="noreferrer noopener"
                      className="min-w-0 truncate text-[13px] font-medium text-[var(--color-text-accent)] hover:underline"
                      title={hit.title}
                    >
                      {hit.title || hit.url}
                    </a>
                  ) : (
                    <span className="min-w-0 truncate text-[13px] font-medium text-[var(--color-text-primary)]">
                      {hit.title || '(untitled)'}
                    </span>
                  )}
                </div>
                {hit.url && (
                  <div
                    className="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
                    title={hit.url}
                  >
                    {hit.url}
                  </div>
                )}
                {hit.snippet && (
                  <div className="mt-1 text-[12px] leading-relaxed text-[var(--color-text-secondary)] line-clamp-2">
                    {hit.snippet}
                  </div>
                )}
              </div>
            </div>
          </li>
        ))}
      </ol>
    </div>
  )
}
