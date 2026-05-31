// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ToolViewProps } from './ToolViewProps'
import { useTranslation } from '../../../i18n'

type TodoStatus = 'pending' | 'in_progress' | 'completed' | 'cancelled'

type TodoEntry = {
  id: string
  content: string
  status: TodoStatus
  priority?: string
}

function normalizeStatus(raw: unknown): TodoStatus {
  if (typeof raw !== 'string') return 'pending'
  const v = raw.trim().toLowerCase()
  if (v === 'completed' || v === 'done' || v === 'finished') return 'completed'
  if (v === 'in_progress' || v === 'in-progress' || v === 'inprogress' || v === 'doing')
    return 'in_progress'
  if (v === 'cancelled' || v === 'canceled' || v === 'skipped') return 'cancelled'
  return 'pending'
}

function readTodos(input: unknown): TodoEntry[] {
  if (!input || typeof input !== 'object') return []
  const todosRaw = (input as Record<string, unknown>).todos
  if (!Array.isArray(todosRaw)) return []
  const out: TodoEntry[] = []
  for (const raw of todosRaw) {
    if (!raw || typeof raw !== 'object') continue
    const obj = raw as Record<string, unknown>
    const id = typeof obj.id === 'string' ? obj.id : ''
    const content =
      typeof obj.content === 'string'
        ? obj.content
        : typeof obj.title === 'string'
          ? obj.title
          : ''
    if (!content) continue
    out.push({
      id: id || String(out.length + 1),
      content,
      status: normalizeStatus(obj.status),
      priority: typeof obj.priority === 'string' ? obj.priority : undefined,
    })
  }
  return out
}

function dotClass(status: TodoStatus): string {
  switch (status) {
    case 'completed':
      return 'border-[var(--color-success)] bg-[var(--color-success)]'
    case 'in_progress':
      return 'border-[var(--color-warning)] bg-[var(--color-warning)]/30'
    case 'cancelled':
      return 'border-[var(--color-text-tertiary)] bg-[var(--color-text-tertiary)]/30'
    case 'pending':
    default:
      return 'border-[var(--color-outline-variant)] bg-transparent'
  }
}

function textClass(status: TodoStatus): string {
  switch (status) {
    case 'completed':
      return 'line-through text-[var(--color-text-tertiary)]'
    case 'cancelled':
      return 'line-through text-[var(--color-text-tertiary)] opacity-70'
    case 'in_progress':
      return 'text-[var(--color-text-primary)] font-medium'
    case 'pending':
    default:
      return 'text-[var(--color-text-primary)]'
  }
}

export function TodoListHeader({ input }: ToolViewProps) {
  const t = useTranslation()
  const todos = readTodos(input)
  const total = todos.length
  const completed = todos.filter((todo) => todo.status === 'completed').length
  const inProgress = todos.filter((todo) => todo.status === 'in_progress').length
  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 text-[12px] text-[var(--color-text-secondary)]">
      <span className="truncate">
        {total > 0
          ? t('tool.todo.summary', { total, completed })
          : t('tool.todo.empty')}
      </span>
      {inProgress > 0 && (
        <span className="shrink-0 rounded-full bg-[var(--color-warning)]/15 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-warning)]">
          {t('tool.todo.inProgressBadge', { count: inProgress })}
        </span>
      )}
    </span>
  )
}

export function TodoListDetail({ input }: ToolViewProps) {
  const t = useTranslation()
  const todos = readTodos(input)
  if (todos.length === 0) {
    return (
      <div className="text-[11px] text-[var(--color-text-tertiary)]">
        {t('tool.todo.emptyHint')}
      </div>
    )
  }
  return (
    <ul className="space-y-1">
      {todos.map((todo) => (
        <li key={todo.id} className="flex items-start gap-2 text-[12px] leading-snug">
          <span
            className={`shrink-0 mt-[3px] inline-flex items-center justify-center h-3 w-3 rounded-full border ${dotClass(todo.status)}`}
            aria-label={todo.status}
          />
          <span className={`min-w-0 flex-1 ${textClass(todo.status)}`}>
            {todo.content}
          </span>
          {todo.priority && (
            <span className="shrink-0 rounded-full border border-[var(--color-border)] px-1.5 py-0.5 font-[var(--font-mono)] text-[9px] text-[var(--color-text-tertiary)]">
              {todo.priority}
            </span>
          )}
        </li>
      ))}
    </ul>
  )
}
