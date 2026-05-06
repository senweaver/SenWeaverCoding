import type { ToolViewProps } from './ToolViewProps'
import { TerminalChrome } from '../TerminalChrome'
import { CodeViewer } from '../CodeViewer'
import { extractCommand, extractTextContent, firstWord, truncate } from '../../../utils/toolFormatters'

export function ExecHeader({ input }: ToolViewProps) {
  const command = extractCommand(input)
  if (!command) {
    return (
      <span className="min-w-0 flex-1 truncate text-[11px] text-[var(--color-text-tertiary)]">
        (no command)
      </span>
    )
  }
  const head = firstWord(command)
  return (
    <span
      className="flex min-w-0 flex-1 items-baseline gap-2 truncate text-[12px] text-[var(--color-text-secondary)]"
      title={command}
    >
      {head && (
        <span className="font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {head}
        </span>
      )}
      <span className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
        {truncate(command.replace(/\s+/g, ' '), 120)}
      </span>
    </span>
  )
}

export function ExecDetail({ input, result }: ToolViewProps) {
  const command = extractCommand(input)
  const text = result ? extractTextContent(result.content) : ''

  return (
    <div className="space-y-2">
      {command && (
        <TerminalChrome title={firstWord(command) || 'shell'}>
          <div className="px-3 py-2 font-[var(--font-mono)] text-[11px] leading-[1.45] text-[var(--color-terminal-fg)] whitespace-pre-wrap break-words">
            <span className="text-[var(--color-terminal-accent)]">$</span> {command}
          </div>
        </TerminalChrome>
      )}
      {text && (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <CodeViewer code={text} language="plaintext" maxLines={18} />
        </div>
      )}
    </div>
  )
}
