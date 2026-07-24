// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { diffLines } from 'diff'
import type { ToolViewProps } from './ToolViewProps'
import { DiffViewer } from '../DiffViewer'
import { CodeViewer } from '../CodeViewer'
import { CopyButton } from '../../shared/CopyButton'
import { useTranslation } from '../../../i18n'
import {
  extractTextContent,
  resolveEditTarget,
  type EditTarget,
} from '../../../utils/toolFormatters'

function readString(input: unknown, key: string): string {
  if (!input || typeof input !== 'object') return ''
  const v = (input as Record<string, unknown>)[key]
  return typeof v === 'string' ? v : ''
}

function langFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    ts: 'typescript', tsx: 'tsx', js: 'javascript', jsx: 'jsx',
    py: 'python', rs: 'rust', go: 'go', rb: 'ruby', java: 'java',
    c: 'c', h: 'c', cpp: 'cpp', cc: 'cpp', cs: 'csharp',
    json: 'json', yaml: 'yaml', yml: 'yaml', toml: 'toml',
    md: 'markdown', css: 'css', scss: 'scss', html: 'html', xml: 'xml',
    sql: 'sql', sh: 'bash', bash: 'bash', zsh: 'bash',
  }
  return map[ext] || 'plaintext'
}

type MultiEditEntry = {
  oldString: string
  newString: string
  replaceAll: boolean
  path?: string
}

function readMultiEditEntries(input: unknown): MultiEditEntry[] {
  if (!input || typeof input !== 'object') return []
  const edits = (input as Record<string, unknown>).edits
  if (!Array.isArray(edits)) return []
  const entries: MultiEditEntry[] = []
  for (const raw of edits) {
    if (!raw || typeof raw !== 'object') continue
    const obj = raw as Record<string, unknown>
    const oldRaw = obj.old_string
    const newRaw = obj.new_string
    const replaceAllRaw = obj.replace_all
    const pathRaw = obj.path ?? obj.file_path
    if (typeof oldRaw !== 'string' && typeof newRaw !== 'string') continue
    entries.push({
      oldString: typeof oldRaw === 'string' ? oldRaw : '',
      newString: typeof newRaw === 'string' ? newRaw : '',
      replaceAll: replaceAllRaw === true,
      path: typeof pathRaw === 'string' && pathRaw.trim() ? pathRaw : undefined,
    })
  }
  return entries
}

function changedLineCount(a: string, b: string): number {
  let changed = 0
  for (const part of diffLines(a, b)) {
    if (part.added || part.removed) changed += part.count ?? 0
  }
  return changed
}

const EDIT_STYLE_NAMES = new Set([
  'file_edit',
  'Edit',
  'multi_edit',
  'MultiEdit',
  'notebook_edit',
  'NotebookEdit',
  'glob_edit',
])

function parseAdditionsDeletions(
  result: ToolViewProps['result'],
): { adds: number; dels: number } | null {
  if (!result || result.isError) return null
  const text = extractTextContent(result.content)
  if (!text) return null

  const m = text.match(/(?:^|[^\w])(\d+)\s+addition[s]?(?:[^,]*?,)?\s*(\d+)\s+deletion/i)
  if (m) {
    return { adds: parseInt(m[1] || '0', 10), dels: parseInt(m[2] || '0', 10) }
  }
  const plusMinus = text.match(/(?:^|\s)\+(\d+)\s*\/\s*-(\d+)\b/)
  if (plusMinus) {
    return { adds: parseInt(plusMinus[1] || '0', 10), dels: parseInt(plusMinus[2] || '0', 10) }
  }
  if (/^[+\- ]/m.test(text)) {
    let adds = 0
    let dels = 0
    for (const line of text.split('\n')) {
      if (line.startsWith('+') && !line.startsWith('+++')) adds++
      else if (line.startsWith('-') && !line.startsWith('---')) dels++
    }
    if (adds + dels > 0) return { adds, dels }
  }
  return null
}

function canonicalPathFor(
  target: EditTarget,
  toolName: string,
  translate: ReturnType<typeof useTranslation>,
): string {
  switch (target.kind) {
    case 'path':
      return target.isWorkspaceRoot
        ? translate('tool.list.workspaceRoot')
        : target.full
    case 'glob':
      return target.pattern
    case 'multi':
      return target.first
    default:
      return toolName
  }
}

export function EditHeader({ toolName, input, result }: ToolViewProps) {
  const t = useTranslation()
  const target = resolveEditTarget(input, toolName)
  const oldStr = readString(input, 'old_string')
  const newStr = readString(input, 'new_string')
  const content = readString(input, 'content')

  let badge = ''
  const fromResult = parseAdditionsDeletions(result)
  if (fromResult) {
    badge = `+${fromResult.adds} / -${fromResult.dels}`
  } else if (toolName === 'file_write' || toolName === 'Write' || toolName === 'file_create') {
    const lines = content ? content.split('\n').length : 0
    badge = lines > 0 ? `+${lines}` : ''
  } else if (EDIT_STYLE_NAMES.has(toolName) && oldStr && newStr) {
    const changed = changedLineCount(oldStr, newStr)
    badge = changed > 0 ? `~${changed}` : ''
  }

  const pathForCopy =
    target.kind === 'path'
      ? target.full
      : target.kind === 'glob'
        ? target.pattern
        : target.kind === 'multi'
          ? target.paths.join('\n')
          : ''

  const titleAttr =
    target.kind === 'multi'
      ? target.paths.join('\n')
      : target.kind === 'glob'
        ? target.pattern
        : target.kind === 'path'
          ? target.full
          : toolName

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      <span
        className="min-w-0 flex-1 flex items-baseline gap-0 truncate"
        title={titleAttr}
      >
        <EditTargetLabel target={target} toolName={toolName} t={t} />
      </span>
      {badge && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
          {badge}
        </span>
      )}
      {target.kind === 'multi' && target.count > 1 && (
        <span
          className="shrink-0 rounded-full border border-[var(--color-border)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]"
          title={target.paths.join('\n')}
        >
          {t('tool.edit.multiFiles', { count: target.count })}
        </span>
      )}
      {pathForCopy && (
        <CopyButton
          text={pathForCopy}
          className="shrink-0 rounded-md border border-[var(--color-border)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          onClick={(e) => e.stopPropagation()}
        />
      )}
    </span>
  )
}

type LabelProps = {
  target: EditTarget
  toolName: string
  t: ReturnType<typeof useTranslation>
}

function EditTargetLabel({ target, toolName, t }: LabelProps) {
  if (target.kind === 'path') {
    if (target.isWorkspaceRoot) {
      return (
        <span className="truncate font-[var(--font-mono)] text-[12px] italic text-[var(--color-text-tertiary)]">
          {t('tool.list.workspaceRoot')}
        </span>
      )
    }
    if (target.dir) {
      return (
        <>
          <span className="min-w-0 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-tertiary)]">
            {target.dir}
            {target.separator}
          </span>
          <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
            {target.tail}
          </span>
        </>
      )
    }
    return (
      <span className="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {target.tail || target.full}
      </span>
    )
  }

  if (target.kind === 'glob') {
    return (
      <span className="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {target.pattern}
      </span>
    )
  }

  if (target.kind === 'multi') {
    const first = target.first || toolName
    return (
      <span className="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {first}
      </span>
    )
  }

  return (
    <span className="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-tertiary)]">
      {toolName}
    </span>
  )
}

export function EditDetail({ toolName, input, result, isStreaming }: ToolViewProps) {
  const t = useTranslation()
  const target = resolveEditTarget(input, toolName)
  const path = canonicalPathFor(target, toolName, t)
  const oldStr = readString(input, 'old_string')
  const newStr = readString(input, 'new_string')
  const content = readString(input, 'content')

  const isMultiEdit = toolName === 'multi_edit' || toolName === 'MultiEdit'
  const isNotebookEdit = toolName === 'notebook_edit' || toolName === 'NotebookEdit'
  const isPatchApply = toolName === 'patch_apply'

  if (isPatchApply) {
    const patch = readString(input, 'patch')
    if (patch) {
      return (
        <CodeViewer
          code={patch}
          language="diff"
          maxLines={28}
          showLineNumbers={false}
        />
      )
    }
  }

  if (isNotebookEdit) {
    const newSource = readString(input, 'new_source')
    const oldSource = readString(input, 'old_source') || readString(input, 'old_string')
    if (newSource || oldSource) {
      return (
        <DiffViewer filePath={path} oldString={oldSource} newString={newSource} />
      )
    }
  }

  if (isMultiEdit) {
    const entries = readMultiEditEntries(input)
    if (entries.length > 0) {
      return (
        <div className="space-y-2">
          {entries.map((entry, idx) => (
            <DiffViewer
              key={`${path}-${idx}`}
              filePath={entry.path || path}
              oldString={entry.oldString}
              newString={entry.newString}
            />
          ))}
        </div>
      )
    }
  }

  if (toolName === 'glob_edit') {
    const text = result ? extractTextContent(result.content) : ''
    return (
      <div className="space-y-2">
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 text-[11px] text-[var(--color-text-secondary)]">
          <div className="flex items-center gap-1.5 font-[var(--font-mono)]">
            <span className="shrink-0 text-[var(--color-text-tertiary)]">
              {t('tool.edit.globPattern')}
            </span>
            <span className="min-w-0 truncate text-[var(--color-text-primary)]" title={path}>
              {path}
            </span>
          </div>
          {(oldStr || newStr) && (
            <div className="mt-1 space-y-0.5 font-[var(--font-mono)]">
              <div
                className="truncate text-[var(--color-error)]"
                title={oldStr}
              >
                {`- ${oldStr}`}
              </div>
              <div
                className="truncate text-[var(--color-success)]"
                title={newStr}
              >
                {`+ ${newStr}`}
              </div>
            </div>
          )}
        </div>
        {text && <CodeViewer code={text} language="plaintext" maxLines={14} />}
      </div>
    )
  }

  if (EDIT_STYLE_NAMES.has(toolName) && (oldStr || newStr)) {
    return <DiffViewer filePath={path} oldString={oldStr} newString={newStr} />
  }
  if ((toolName === 'file_write' || toolName === 'Write' || toolName === 'file_create') && content) {
    return (
      <CodeViewer
        code={content}
        language={langFromPath(path)}
        maxLines={40}
        showLineNumbers={false}
      />
    )
  }

  const text = result ? extractTextContent(result.content) : ''

  if (!result && isStreaming) {
    return (
      <div
        className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
        aria-live="polite"
      >
        <span className="inline-flex items-center gap-2">
          <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-[var(--color-primary)]" />
          <span className="truncate">
            {path
              ? t('tool.edit.executing', { path })
              : t('tool.edit.executingGeneric')}
          </span>
        </span>
      </div>
    )
  }

  const showMeta =
    target.kind !== 'unknown' &&
    !(target.kind === 'path' && !target.full)

  return (
    <div className="space-y-2">
      {showMeta && (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
          {target.kind === 'multi' ? (
            <ul className="space-y-0.5">
              {target.paths.map((p, i) => (
                <li key={`${p}-${i}`} className="truncate" title={p}>
                  {p}
                </li>
              ))}
            </ul>
          ) : (
            <span className="truncate">{path}</span>
          )}
        </div>
      )}
      {text && <CodeViewer code={text} language="plaintext" maxLines={14} />}
    </div>
  )
}
