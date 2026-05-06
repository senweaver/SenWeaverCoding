import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import {
  extractTextContent,
  truncate,
} from '../../../utils/toolFormatters'

function readString(input: unknown, keys: string[]): string {
  if (!input || typeof input !== 'object') return ''
  const obj = input as Record<string, unknown>
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.trim()) return v
  }
  return ''
}

export function ModelHeader({ toolName, input }: ToolViewProps) {
  const from = readString(input, ['from', 'previous', 'old', 'old_model', 'source'])
  const to = readString(input, ['to', 'next', 'new', 'new_model', 'target', 'model'])
  const provider = readString(input, ['provider', 'vendor'])
  const action = readString(input, ['action', 'op', 'method', 'kind'])
  const parts: string[] = []
  if (action) parts.push(action)
  if (provider) parts.push(provider)
  const label = parts.length > 0 ? parts.join(' · ') : toolName
  if (from && to) {
    return (
      <span className="min-w-0 flex-1 flex items-baseline gap-1.5 truncate text-[12px] text-[var(--color-text-secondary)]">
        <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {truncate(from, 32)}
        </span>
        <span className="shrink-0 text-[11px] text-[var(--color-text-tertiary)]">→</span>
        <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {truncate(to, 32)}
        </span>
      </span>
    )
  }
  return (
    <span
      className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
      title={label}
    >
      {truncate(label, 80)}
    </span>
  )
}

export function ModelDetail({ input, result }: ToolViewProps) {
  const from = readString(input, ['from', 'previous', 'old', 'old_model', 'source'])
  const to = readString(input, ['to', 'next', 'new', 'new_model', 'target', 'model'])
  const provider = readString(input, ['provider', 'vendor'])
  const reason = readString(input, ['reason', 'why'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const hasStructured = Boolean(from || to || provider || reason)

  return (
    <div className="space-y-2">
      {hasStructured ? (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 space-y-1 font-[var(--font-mono)] text-[11px]">
          {(from || to) && (
            <div className="flex items-center gap-2">
              <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                Swap
              </span>
              <span className="truncate text-[var(--color-text-secondary)]">
                {from || '—'} → {to || '—'}
              </span>
            </div>
          )}
          {provider && (
            <div className="flex items-center gap-2">
              <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                Provider
              </span>
              <span className="truncate text-[var(--color-text-secondary)]">
                {provider}
              </span>
            </div>
          )}
          {reason && (
            <div className="flex items-start gap-2">
              <span className="w-16 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                Reason
              </span>
              <span className="whitespace-pre-wrap break-words text-[var(--color-text-secondary)]">
                {reason}
              </span>
            </div>
          )}
        </div>
      ) : (
        <CodeViewer code={inputJson} language="json" maxLines={10} />
      )}
      {text && (
        <CodeViewer code={text} language="plaintext" maxLines={14} />
      )}
    </div>
  )
}
