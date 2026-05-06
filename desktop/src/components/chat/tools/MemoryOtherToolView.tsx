import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
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

export function MemoryOtherHeader({ toolName, input }: ToolViewProps) {
  const action =
    readString(input, ['action', 'op', 'kind', 'method']) ||
    toolName.replace(/^memory_/, '') ||
    toolName
  const key = readString(input, ['key', 'name', 'label', 'topic', 'path'])
  const preview = readString(input, ['value', 'fact', 'note', 'content'])
  return (
    <span
      className="min-w-0 flex-1 flex items-baseline gap-2 truncate text-[12px] text-[var(--color-text-secondary)]"
      title={preview || key || action}
    >
      <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {truncate(action, 22)}
      </span>
      {key && (
        <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {truncate(key, 32)}
        </span>
      )}
      {preview && (
        <span className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {truncate(preview.replace(/\s+/g, ' '), 100)}
        </span>
      )}
    </span>
  )
}

export function MemoryOtherDetail({ input, result }: ToolViewProps) {
  const key = readString(input, ['key', 'name', 'label', 'topic', 'path'])
  const value = readString(input, ['value', 'fact', 'note', 'content'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const hasStructured = Boolean(key || value)

  return (
    <div className="space-y-2">
      {hasStructured ? (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] space-y-1">
          {key && (
            <div className="flex items-center gap-2">
              <span className="text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                Key
              </span>
              <span className="truncate text-[var(--color-text-secondary)]">
                {key}
              </span>
            </div>
          )}
          {value && (
            <div className="whitespace-pre-wrap break-words text-[var(--color-text-secondary)]">
              {value}
            </div>
          )}
        </div>
      ) : (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>Input</span>
            <CopyButton
              text={inputJson}
              className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <CodeViewer code={inputJson} language="json" maxLines={10} />
        </div>
      )}
      {text && (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <CodeViewer code={text} language="plaintext" maxLines={14} />
        </div>
      )}
    </div>
  )
}
