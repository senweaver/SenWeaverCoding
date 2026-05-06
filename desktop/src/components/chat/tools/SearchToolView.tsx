import { useMemo, type ReactNode } from 'react'
import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import {
  extractQuery,
  extractTextContent,
  parseSearchHits,
  truncate,
  type SearchHit,
} from '../../../utils/toolFormatters'

const POPOVER_PREVIEW_LIMIT = 12

export function SearchHeader({ input, result }: ToolViewProps) {
  const query = extractQuery(input) || (input && typeof input === 'object'
    ? String((input as Record<string, unknown>).path ?? '')
    : '')
  const hits = useMemo(
    () => (result ? parseSearchHits(extractTextContent(result.content)) : []),
    [result],
  )

  return (
    <span
      className="flex min-w-0 flex-1 items-center gap-2 truncate text-[12px] text-[var(--color-text-secondary)]"
    >
      <span className="min-w-0 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {query || '(empty pattern)'}
      </span>
      {hits.length > 0 && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-tertiary)]">
          {hits.length === 1 ? '1 hit' : `${hits.length} hits`}
        </span>
      )}
    </span>
  )
}

export function getSearchHoverContent({ result }: ToolViewProps): ReactNode | null {
  if (!result) return null
  const text = extractTextContent(result.content)
  if (!text) return null
  const hits = parseSearchHits(text)
  if (hits.length === 0) return null
  return <HitsPopoverContent hits={hits} />
}

export function SearchDetail({ input, result }: ToolViewProps) {
  const query = extractQuery(input)
  const text = result ? extractTextContent(result.content) : ''
  const hits = useMemo(() => parseSearchHits(text, 200), [text])

  return (
    <div className="space-y-2">
      {query && (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
          {query}
        </div>
      )}
      {hits.length > 0 ? (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <ul className="max-h-[280px] divide-y divide-[var(--color-border)]/40 overflow-y-auto">
            {hits.map((hit, idx) => (
              <li key={idx} className="px-3 py-1.5">
                <HitRow hit={hit} />
              </li>
            ))}
          </ul>
        </div>
      ) : text ? (
        <CodeViewer code={text} language="plaintext" maxLines={14} />
      ) : null}
    </div>
  )
}

function HitsPopoverContent({ hits }: { hits: SearchHit[] }) {
  const visible = hits.slice(0, POPOVER_PREVIEW_LIMIT)
  const overflow = hits.length - visible.length
  return (
    <div className="max-h-[260px] overflow-y-auto py-1">
      <ul className="divide-y divide-[var(--color-border)]/40">
        {visible.map((hit, idx) => (
          <li key={idx} className="px-2 py-1.5">
            <HitRow hit={hit} />
          </li>
        ))}
      </ul>
      {overflow > 0 && (
        <div className="border-t border-[var(--color-border)]/40 px-2 py-1 text-[10px] text-[var(--color-text-tertiary)]">
          +{overflow} more
        </div>
      )}
    </div>
  )
}

function HitRow({ hit }: { hit: SearchHit }) {
  const location = [hit.file, hit.line].filter(Boolean).join(':')
  return (
    <div className="space-y-0.5">
      {location && (
        <div
          className="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]"
          title={location}
        >
          {location}
        </div>
      )}
      <div className="line-clamp-2 break-words font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
        {truncate(hit.preview, 240)}
      </div>
    </div>
  )
}
