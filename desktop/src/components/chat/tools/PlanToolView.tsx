// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer'

type Todo = {
  content?: unknown
  status?: unknown
  activeForm?: unknown
}

function extractTodos(input: unknown): Todo[] {
  if (!input || typeof input !== 'object') return []
  const v = (input as Record<string, unknown>).todos
  return Array.isArray(v) ? (v as Todo[]) : []
}

function extractMarkdown(input: unknown): string {
  if (!input || typeof input !== 'object') return ''
  const obj = input as Record<string, unknown>
  for (const key of ['plan', 'markdown', 'content', 'body', 'text']) {
    const v = obj[key]
    if (typeof v === 'string' && v.trim()) return v
  }
  return ''
}

function statusIcon(status: unknown): string {
  if (status === 'completed') return '✔'
  if (status === 'in_progress') return '◐'
  if (status === 'cancelled') return '✕'
  return '○'
}

export function PlanHeader({ toolName, input }: ToolViewProps) {
  const todos = extractTodos(input)
  if (todos.length > 0) {
    const done = todos.filter((t) => t.status === 'completed').length
    return (
      <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--color-text-secondary)]">
        {done}/{todos.length} todos
      </span>
    )
  }
  const md = extractMarkdown(input)
  if (md) {
    const firstLine = md.split('\n').find((l) => l.trim()) ?? ''
    return (
      <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
        {firstLine.replace(/^#+\s*/, '').slice(0, 80)}
      </span>
    )
  }
  return (
    <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--color-text-tertiary)]">
      {toolName}
    </span>
  )
}

export function PlanDetail({ input }: ToolViewProps) {
  const todos = extractTodos(input)
  const md = extractMarkdown(input)

  if (todos.length > 0) {
    return (
      <ul className="space-y-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-[12px] text-[var(--color-text-secondary)]">
        {todos.map((todo, idx) => (
          <li key={idx} className="flex items-start gap-2">
            <span
              className={`shrink-0 font-[var(--font-mono)] ${
                todo.status === 'completed'
                  ? 'text-[var(--color-success)]'
                  : todo.status === 'in_progress'
                    ? 'text-[var(--color-warning)]'
                    : 'text-[var(--color-text-tertiary)]'
              }`}
            >
              {statusIcon(todo.status)}
            </span>
            <span
              className={`leading-snug ${
                todo.status === 'completed'
                  ? 'text-[var(--color-text-tertiary)] line-through'
                  : 'text-[var(--color-text-secondary)]'
              }`}
            >
              {typeof todo.content === 'string' ? todo.content : JSON.stringify(todo.content)}
            </span>
          </li>
        ))}
      </ul>
    )
  }

  if (md) {
    return (
      <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2">
        <MarkdownRenderer content={md} />
      </div>
    )
  }

  return null
}
