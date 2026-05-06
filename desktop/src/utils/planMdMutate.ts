

import type { PlanTodo } from './parsePlanMd'

function yamlScalar(value: string): string {
  if (value === '') return '""'

  const needsQuoting =
    /[:#\n\r\t"'&*!|>%@`,\[\]\{\}]/.test(value) ||
    /^\s|\s$/.test(value) ||
    /^[-?*&!|>'"#%@`]/.test(value)
  if (!needsQuoting) return value

  return JSON.stringify(value)
}

export function buildTodosYamlBlock(todos: PlanTodo[]): string {
  if (todos.length === 0) return 'todos: []'
  const lines: string[] = ['todos:']
  for (const t of todos) {
    lines.push(`  - id: ${yamlScalar(t.id)}`)
    lines.push(`    content: ${yamlScalar(t.content)}`)
    lines.push(`    status: ${t.status}`)
  }
  return lines.join('\n')
}

export function rewritePlanMarkdownTodos(
  markdown: string,
  todos: PlanTodo[],
): string {
  if (!markdown || !markdown.startsWith('---')) return markdown

  const closeIdx = markdown.indexOf('\n---', 3)
  if (closeIdx < 0) return markdown

  const yamlPart = markdown.slice(0, closeIdx)
  const rest = markdown.slice(closeIdx)

  const lines = yamlPart.split('\n')
  const start = lines.findIndex((l) => l.startsWith('todos:'))
  if (start < 0) return markdown

  let end = lines.length
  for (let i = start + 1; i < lines.length; i++) {
    const l = lines[i] ?? ''
    if (l.length === 0) continue
    if (!l.startsWith(' ') && !l.startsWith('\t')) {
      end = i
      break
    }
  }

  const newBlockLines = buildTodosYamlBlock(todos).split('\n')
  const next = [
    ...lines.slice(0, start),
    ...newBlockLines,
    ...lines.slice(end),
  ]
  return next.join('\n') + rest
}
