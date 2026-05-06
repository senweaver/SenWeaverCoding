import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { extractQuery, extractTextContent, truncate } from '../../../utils/toolFormatters'

export function MemoryRecallHeader({ input }: ToolViewProps) {
  const query = extractQuery(input)
  return (
    <span
      className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
      title={query}
    >
      {truncate(query || '(memory)', 80)}
    </span>
  )
}

export function MemoryRecallDetail({ input, result }: ToolViewProps) {
  const query = extractQuery(input)
  const text = result ? extractTextContent(result.content) : ''

  return (
    <div className="space-y-2">
      {query && (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
          {query}
        </div>
      )}
      {text && <CodeViewer code={text} language="plaintext" maxLines={16} />}
    </div>
  )
}
