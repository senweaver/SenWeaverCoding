// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding

import { diffLines } from 'diff'
import type * as MonacoNs from 'monaco-editor'

const DECORATION_FADE_MS = 5_000

const decorationState: WeakMap<
  MonacoNs.editor.IStandaloneCodeEditor,
  { ids: string[]; timer: number | null }
> = new WeakMap()

export type ApplyAiDecorationsArgs = {
  monaco: typeof MonacoNs
  editor: MonacoNs.editor.IStandaloneCodeEditor
  previousContent: string
  currentContent: string
}

export type AiDecorationResult = {

  changedLineCount: number
}

export function applyAiDecorations({
  monaco,
  editor,
  previousContent,
  currentContent,
}: ApplyAiDecorationsArgs): AiDecorationResult {
  if (previousContent === currentContent) {
    return { changedLineCount: 0 }
  }

  const ranges = computeChangedLineRanges(previousContent, currentContent)
  if (ranges.length === 0) {
    return { changedLineCount: 0 }
  }

  const newDecorations: MonacoNs.editor.IModelDeltaDecoration[] = ranges.map(
    (range) => ({
      range: new monaco.Range(range.startLine, 1, range.endLine, 1),
      options: {
        isWholeLine: true,
        className: 'sen-ai-edit-line',
        linesDecorationsClassName: 'sen-ai-edit-gutter',
        overviewRuler: {
          color: 'rgba(34, 197, 94, 0.6)',
          position: monaco.editor.OverviewRulerLane.Left,
        },
        minimap: {
          color: 'rgba(34, 197, 94, 0.45)',
          position: monaco.editor.MinimapPosition.Inline,
        },
      },
    }),
  )

  const existing = decorationState.get(editor)
  const oldIds = existing?.ids ?? []
  if (existing?.timer !== null && existing?.timer !== undefined) {
    window.clearTimeout(existing.timer)
  }

  const newIds = editor.deltaDecorations(oldIds, newDecorations)

  const first = ranges[0]
  if (first) {
    editor.revealLineInCenterIfOutsideViewport(first.startLine)
  }

  const timer = window.setTimeout(() => {
    editor.deltaDecorations(newIds, [])
    decorationState.delete(editor)
  }, DECORATION_FADE_MS)

  decorationState.set(editor, { ids: newIds, timer })

  const total = ranges.reduce(
    (acc, r) => acc + (r.endLine - r.startLine + 1),
    0,
  )
  return { changedLineCount: total }
}

type ChangedLineRange = {
  startLine: number
  endLine: number
}

function computeChangedLineRanges(
  prev: string,
  curr: string,
): ChangedLineRange[] {
  const chunks = diffLines(prev, curr)
  const out: ChangedLineRange[] = []
  let lineCursor = 1

  for (const chunk of chunks) {
    const lineCount = chunk.count ?? chunk.value.split('\n').length - 1
    if (chunk.added) {
      const start = lineCursor
      const end = lineCursor + Math.max(0, lineCount - 1)
      out.push({ startLine: start, endLine: end })
      lineCursor = end + 1
    } else if (chunk.removed) {

      const target = Math.max(1, lineCursor - 1)
      out.push({ startLine: target, endLine: target })
    } else {
      lineCursor += lineCount
    }
  }

  out.sort((a, b) => a.startLine - b.startLine)
  const merged: ChangedLineRange[] = []
  for (const range of out) {
    const last = merged[merged.length - 1]
    if (last && range.startLine <= last.endLine + 1) {
      last.endLine = Math.max(last.endLine, range.endLine)
    } else {
      merged.push({ ...range })
    }
  }
  return merged
}
