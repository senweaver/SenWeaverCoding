// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export type PlanTodoStatus = 'pending' | 'in_progress' | 'completed' | 'cancelled'

export type PlanTodo = {
  id: string
  content: string
  status: PlanTodoStatus
}

export type ParsedPlan = {
  name: string
  overview: string
  todos: PlanTodo[]
  body: string

  title: string
}

const stripFeff = (s: string) => (s.startsWith('\u{feff}') ? s.slice(1) : s)

function unquote(value: string): string {
  const trimmed = value.trim()
  if (
    (trimmed.startsWith('"') && trimmed.endsWith('"')) ||
    (trimmed.startsWith("'") && trimmed.endsWith("'"))
  ) {
    return trimmed.slice(1, -1)
  }
  return trimmed
}

function normalizeStatus(raw: string): PlanTodoStatus {
  const v = raw.trim().toLowerCase().replace(/^"|"$|^'|'$/g, '')
  switch (v) {
    case 'completed':
    case 'done':
    case 'finished':
      return 'completed'
    case 'in_progress':
    case 'in-progress':
    case 'inprogress':
    case 'doing':
      return 'in_progress'
    case 'cancelled':
    case 'canceled':
    case 'skipped':
      return 'cancelled'
    default:
      return 'pending'
  }
}

function parseTodosBlock(yamlBody: string): PlanTodo[] {
  const lines = yamlBody.split('\n')
  let inTodos = false
  let current: Partial<PlanTodo> | null = null
  const out: PlanTodo[] = []

  const pushCurrent = () => {
    if (current && current.id !== undefined && current.content !== undefined) {
      out.push({
        id: String(current.id),
        content: String(current.content),
        status: (current.status as PlanTodoStatus) ?? 'pending',
      })
    }
    current = null
  }

  for (const rawLine of lines) {
    const line = rawLine.replace(/\s+$/, '')
    const trimmed = line.trim()

    if (!inTodos) {
      if (line.startsWith('todos:') && line.trimStart() === line) {
        inTodos = true
      }
      continue
    }

    if (!line.startsWith(' ') && !line.startsWith('\t') && trimmed.length > 0) {
      pushCurrent()
      inTodos = false
      continue
    }

    const idMatch = trimmed.match(/^- id:\s*(.*)$/)
    if (idMatch) {
      pushCurrent()
      current = { id: unquote(idMatch[1] ?? ''), status: 'pending' }
      continue
    }

    const contentMatch = trimmed.match(/^content:\s*(.*)$/)
    if (contentMatch && current) {
      current.content = unquote(contentMatch[1] ?? '')
      continue
    }

    const statusMatch = trimmed.match(/^status:\s*(.*)$/)
    if (statusMatch && current) {
      current.status = normalizeStatus(statusMatch[1] ?? '')
      continue
    }
  }
  pushCurrent()
  return out
}

function readScalar(yamlBody: string, key: string): string {
  for (const line of yamlBody.split('\n')) {
    if (line.startsWith(`${key}:`)) {
      return unquote(line.slice(key.length + 1))
    }
  }
  return ''
}

export function parsePlanMarkdown(content: string): ParsedPlan {
  const text = stripFeff(content ?? '')
  let body = text
  let yamlBody = ''
  if (text.startsWith('---')) {
    const rest = text.slice(3)
    const closeIdx = rest.indexOf('\n---')
    if (closeIdx >= 0) {
      yamlBody = rest.slice(0, closeIdx).replace(/^\n/, '')
      body = rest
        .slice(closeIdx + 4)
        .replace(/^[\r\n]+/, '')
    }
  }

  const name = readScalar(yamlBody, 'name')
  const overview = readScalar(yamlBody, 'overview')
  const todos = parseTodosBlock(yamlBody)

  let title = name
  for (const line of body.split('\n')) {
    const m = line.match(/^#\s+(.+?)\s*$/)
    if (m) {
      title = m[1] ?? name
      break
    }
  }

  return {
    name,
    overview,
    todos,
    body,
    title: title || name || 'Untitled plan',
  }
}

export function parseSavedPlanResult(
  output: string,
): { planPath: string; markdown: string } | null {
  const trimmed = output.trimStart()
  const m = trimmed.match(/^Plan saved to `([^`]+)`/)
  if (!m) return null
  const planPath = m[1] ?? ''
  const newlineIdx = trimmed.indexOf('\n')
  const markdown = newlineIdx >= 0 ? trimmed.slice(newlineIdx + 1).replace(/^\s*\n/, '') : ''
  return { planPath, markdown }
}

export function parseExitPlanModeResult(
  output: string,
): { planPath: string; markdown: string } | null {
  const pathMatch = output.match(/Plan saved to `([^`]+)`/)
  const planPath = pathMatch?.[1] ?? ''

  const begin = output.indexOf('===PLAN_MARKDOWN_BEGIN===')
  const end = output.indexOf('===PLAN_MARKDOWN_END===')
  let markdown = ''
  if (begin >= 0 && end > begin) {
    const start = begin + '===PLAN_MARKDOWN_BEGIN==='.length
    markdown = output.slice(start, end).replace(/^\r?\n/, '').replace(/\r?\n\s*$/, '')
  }

  if (!planPath && !markdown) return null
  return { planPath, markdown }
}

export function planFileNameFromPath(planPath: string): string {
  const norm = planPath.replace(/\\/g, '/')
  const idx = norm.lastIndexOf('/')
  return idx >= 0 ? norm.slice(idx + 1) : norm
}
