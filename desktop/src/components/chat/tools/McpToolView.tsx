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

function splitMcpName(toolName: string): { server: string; tool: string } {
  const idx = toolName.indexOf('__')
  if (idx < 0) return { server: '', tool: toolName }
  return { server: toolName.slice(0, idx), tool: toolName.slice(idx + 2) }
}

export function McpHeader({ toolName, input }: ToolViewProps) {
  const { server, tool } = splitMcpName(toolName)
  const action = readString(input, ['action', 'op', 'method', 'kind', 'tool'])
  const summary =
    readString(input, ['query', 'pattern', 'path', 'url', 'resource', 'uri', 'name'])

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      {server && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-secondary)]">
          {truncate(server, 18)}
        </span>
      )}
      <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {truncate(tool || toolName, 40)}
      </span>
      {action && (
        <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          · {truncate(action, 20)}
        </span>
      )}
      {summary && (
        <span
          className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
          title={summary}
        >
          {truncate(summary, 80)}
        </span>
      )}
    </span>
  )
}

export function McpDetail({ toolName, input, result }: ToolViewProps) {
  const { server, tool } = splitMcpName(toolName)
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  return (
    <div className="space-y-2">
      {server && (
        <div className="flex items-center gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1 font-[var(--font-mono)] text-[11px]">
          <span className="text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            MCP
          </span>
          <span className="text-[var(--color-text-secondary)]">
            {server} / {tool}
          </span>
        </div>
      )}
      <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
          <span>Arguments</span>
          <CopyButton
            text={inputJson}
            className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          />
        </div>
        <CodeViewer code={inputJson} language="json" maxLines={12} />
      </div>
      {text && (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>{result?.isError ? 'Error' : 'Output'}</span>
            <CopyButton
              text={text}
              className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <CodeViewer code={text} language="plaintext" maxLines={18} />
        </div>
      )}
    </div>
  )
}
