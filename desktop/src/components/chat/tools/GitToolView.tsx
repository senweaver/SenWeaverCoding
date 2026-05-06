import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
import { TerminalChrome } from '../TerminalChrome'
import {
  extractPath,
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

export function GitHeader({ toolName, input }: ToolViewProps) {
  const action = readString(input, [
    'action',
    'op',
    'operation',
    'subcommand',
    'command',
    'verb',
  ])
  const repo = readString(input, ['repo', 'repository', 'cwd', 'working_dir'])
    || extractPath(input)
  const worktree = readString(input, ['worktree', 'branch'])
  const parts: string[] = []
  if (action) parts.push(action)
  if (repo) parts.push(repo)
  else if (worktree) parts.push(worktree)
  const label = parts.length > 0 ? parts.join(' · ') : toolName
  return (
    <span
      className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
      title={label}
    >
      {truncate(label, 80)}
    </span>
  )
}

export function GitDetail({ input, result }: ToolViewProps) {
  const action = readString(input, [
    'action',
    'op',
    'operation',
    'subcommand',
    'command',
    'verb',
  ])
  const repo = readString(input, ['repo', 'repository', 'cwd', 'working_dir'])
    || extractPath(input)
  const worktree = readString(input, ['worktree', 'branch'])
  const text = result ? extractTextContent(result.content) : ''
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const metaRows: Array<{ label: string; value: string }> = []
  if (action) metaRows.push({ label: 'Action', value: action })
  if (repo) metaRows.push({ label: 'Repo', value: repo })
  if (worktree) metaRows.push({ label: 'Worktree', value: worktree })

  return (
    <div className="space-y-2">
      {metaRows.length > 0 ? (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5">
          <dl className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-0.5 font-[var(--font-mono)] text-[11px]">
            {metaRows.map((row) => (
              <div key={row.label} className="contents">
                <dt className="text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                  {row.label}
                </dt>
                <dd className="truncate text-[var(--color-text-secondary)]">
                  {row.value}
                </dd>
              </div>
            ))}
          </dl>
        </div>
      ) : (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-1.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            <span>Tool Input</span>
            <CopyButton
              text={inputJson}
              className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[10px] normal-case tracking-normal text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            />
          </div>
          <CodeViewer code={inputJson} language="json" maxLines={10} />
        </div>
      )}
      {text && (
        <TerminalChrome title={action || 'git'}>
          <div className="px-3 py-2 font-[var(--font-mono)] text-[11px] leading-[1.45] text-[var(--color-terminal-fg)] whitespace-pre-wrap break-words">
            {text}
          </div>
        </TerminalChrome>
      )}
    </div>
  )
}
