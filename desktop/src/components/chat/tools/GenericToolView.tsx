import type { ToolViewProps, Translator } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
import { extractTextContent, truncate } from '../../../utils/toolFormatters'

const SUMMARY_KEYS = [
  'path',
  'file_path',
  'target',
  'query',
  'pattern',
  'command',
  'url',
  'name',
  'id',
  'key',
  'topic',
  'label',
] as const

const ACTION_KEYS = [
  'action',
  'op',
  'operation',
  'method',
  'subcommand',
  'kind',
  'verb',
] as const

export function GenericHeader({ toolName, input }: ToolViewProps) {
  const action = readKey(input, ACTION_KEYS)
  const summary = readKey(input, SUMMARY_KEYS) || summarizeInput(input)
  return (
    <span className="min-w-0 flex-1 flex items-baseline gap-2 truncate text-[11px] text-[var(--color-text-tertiary)]">
      <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-secondary)]">
        {toolName}
      </span>
      {action && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-secondary)]">
          {truncate(action, 20)}
        </span>
      )}
      {summary && (
        <span
          className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
          title={summary}
        >
          {truncate(summary, 100)}
        </span>
      )}
    </span>
  )
}

export function GenericDetail({ input, result }: ToolViewProps & { t?: Translator }) {
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const resultText = result ? extractTextContent(result.content) : ''

  return (
    <div className="space-y-2">
      <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
          <span>Input</span>
          <CopyButton
            text={inputJson}
            className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          />
        </div>
        <CodeViewer code={inputJson} language="json" maxLines={12} />
      </div>
      {result && resultText && (
        <div
          className={`overflow-hidden rounded-md border ${
            result.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>{result.isError ? 'Error' : 'Output'}</span>
            <CopyButton
              text={resultText}
              className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <CodeViewer code={resultText} language="plaintext" maxLines={14} />
        </div>
      )}
    </div>
  )
}

function readKey(input: unknown, keys: readonly string[]): string {
  if (!input || typeof input !== 'object') return ''
  const obj = input as Record<string, unknown>
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.trim()) return v
  }
  return ''
}

function summarizeInput(input: unknown): string {
  if (!input) return ''
  if (typeof input === 'string') return truncate(input, 60)
  if (typeof input !== 'object') return String(input)
  const obj = input as Record<string, unknown>
  try {
    return truncate(JSON.stringify(obj), 60)
  } catch {
    return ''
  }
}
