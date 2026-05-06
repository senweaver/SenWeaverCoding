import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import {
  extractTextContent,
  truncate,
} from '../../../utils/toolFormatters'

type Severity = 'error' | 'warning' | 'info' | 'hint'

type Diag = {
  severity: Severity
  message: string
  file?: string
  line?: number
}

function parseDiagnostics(text: string): Diag[] {
  if (!text) return []
  const rows: Diag[] = []
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/\u001b\[[0-9;]*m/g, '').trim()
    if (!line) continue
    const rustc = line.match(
      /^([^\s:][^:\r\n]*):(\d+)(?::\d+)?:\s+(error|warning|note|help|info|hint)\b:?\s*(.*)$/i,
    )
    if (rustc) {
      rows.push({
        severity: normalizeSeverity(rustc[3] ?? ''),
        message: rustc[4] ?? '',
        file: rustc[1] ?? undefined,
        line: Number(rustc[2] ?? '0') || undefined,
      })
      continue
    }
    const bracketed = line.match(
      /^\[(ERROR|WARN|WARNING|INFO|HINT)\]\s+(?:([^\s:]+):(\d+)(?::\d+)?\s+[-—:]\s+)?(.*)$/i,
    )
    if (bracketed) {
      rows.push({
        severity: normalizeSeverity(bracketed[1] ?? ''),
        message: bracketed[4] ?? '',
        file: bracketed[2] ?? undefined,
        line: bracketed[3] ? Number(bracketed[3]) : undefined,
      })
      continue
    }
    rows.push({ severity: 'info', message: line })
  }
  return rows
}

function normalizeSeverity(raw: string): Severity {
  const s = raw.toLowerCase()
  if (s === 'error') return 'error'
  if (s === 'warn' || s === 'warning') return 'warning'
  if (s === 'hint' || s === 'help' || s === 'note') return 'hint'
  return 'info'
}

const SEVERITY_STYLE: Record<Severity, { badge: string; icon: string }> = {
  error: {
    badge: 'bg-[var(--color-error)]/15 text-[var(--color-error)]',
    icon: 'error',
  },
  warning: {
    badge: 'bg-[var(--color-warning)]/15 text-[var(--color-warning)]',
    icon: 'warning',
  },
  info: {
    badge: 'bg-[var(--color-secondary)]/15 text-[var(--color-secondary)]',
    icon: 'info',
  },
  hint: {
    badge: 'bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)]',
    icon: 'lightbulb',
  },
}

export function DiagnosticsHeader({ toolName, result }: ToolViewProps) {
  const text = result ? extractTextContent(result.content) : ''
  const rows = parseDiagnostics(text)
  const errors = rows.filter((r) => r.severity === 'error').length
  const warnings = rows.filter((r) => r.severity === 'warning').length
  const infos = rows.filter((r) => r.severity === 'info' || r.severity === 'hint').length
  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      <span className="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {toolName}
      </span>
      {errors > 0 && (
        <span className={`shrink-0 rounded-full px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] ${SEVERITY_STYLE.error.badge}`}>
          {errors} err
        </span>
      )}
      {warnings > 0 && (
        <span className={`shrink-0 rounded-full px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] ${SEVERITY_STYLE.warning.badge}`}>
          {warnings} warn
        </span>
      )}
      {infos > 0 && errors === 0 && warnings === 0 && (
        <span className={`shrink-0 rounded-full px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] ${SEVERITY_STYLE.info.badge}`}>
          {infos} info
        </span>
      )}
    </span>
  )
}

export function DiagnosticsDetail({ result }: ToolViewProps) {
  const text = result ? extractTextContent(result.content) : ''
  const rows = parseDiagnostics(text)
  if (!rows.length) {
    return text ? (
      <CodeViewer code={text} language="plaintext" maxLines={14} />
    ) : (
      <div className="rounded-md border border-[var(--color-border)]/60 bg-[var(--color-surface-container-low)] px-3 py-2 text-[11px] text-[var(--color-text-tertiary)]">
        No diagnostics.
      </div>
    )
  }
  return (
    <div className="max-h-[320px] overflow-y-auto rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
      <ul className="divide-y divide-[var(--color-border)]/40">
        {rows.map((row, idx) => {
          const style = SEVERITY_STYLE[row.severity]
          return (
            <li key={idx} className="flex items-start gap-2 px-3 py-1.5">
              <span
                className={`material-symbols-outlined mt-0.5 shrink-0 text-[14px] ${style.badge}`}
              >
                {style.icon}
              </span>
              <div className="min-w-0 flex-1">
                <div className="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
                  {row.message || '(no message)'}
                </div>
                {row.file && (
                  <div className="truncate font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
                    {row.file}
                    {row.line ? `:${row.line}` : ''}
                  </div>
                )}
              </div>
              <span
                className={`shrink-0 rounded-full px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] ${style.badge}`}
              >
                {truncate(row.severity, 10)}
              </span>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
