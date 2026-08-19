// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useTranslation } from '../../i18n'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import { DiffViewer } from './DiffViewer'

const PREVIEW_TAIL_LINES = 12

function unescapeJsonFragment(raw: string): string {
  const trimmed = raw.replace(/\\$/, '')
  try {
    return JSON.parse(`"${trimmed}"`) as string
  } catch {
    return trimmed
      .replace(/\\r\\n/g, '\n')
      .replace(/\\n/g, '\n')
      .replace(/\\t/g, '\t')
      .replace(/\\"/g, '"')
      .replace(/\\\\/g, '\\')
  }
}

type PartialEditArgs = {
  path: string | null
  content: string | null
  oldString: string | null
  newString: string | null
}

function extractPartialEditArgs(snapshot: string): PartialEditArgs {
  const pathMatch = snapshot.match(
    /"(?:file_path|path|target_file|notebook_path)"\s*:\s*"((?:[^"\\]|\\.)*)"/,
  )
  const path = pathMatch?.[1] ? unescapeJsonFragment(pathMatch[1]) : null
  const oldMatch = snapshot.match(/"old_string"\s*:\s*"((?:[^"\\]|\\.)*)"/)
  const oldString = oldMatch?.[1] !== undefined ? unescapeJsonFragment(oldMatch[1]) : null
  const newMatch = snapshot.match(/"new_string"\s*:\s*"((?:[^"\\]|\\.)*)/)
  const newString = newMatch?.[1] !== undefined ? unescapeJsonFragment(newMatch[1]) : null
  const contentMatch = snapshot.match(
    /"(?:content|code_edit|new_text|new_source)"\s*:\s*"((?:[^"\\]|\\.)*)/,
  )
  const content = contentMatch?.[1] ? unescapeJsonFragment(contentMatch[1]) : null
  return { path, content, oldString, newString }
}

export function StreamingEditPreview({ sessionId }: { sessionId: string }) {
  const t = useTranslation()
  const streaming = useChatStore(
    (s) => s.sessions[sessionId]?.streamingToolArgs ?? null,
  )
  const chatState = useChatStore(
    (s) => s.sessions[sessionId]?.chatState ?? 'idle',
  )
  const workDir = useSessionStore(
    (s) => s.sessions.find((session) => session.id === sessionId)?.workDir ?? '',
  )
  const baseContentCache = useRef<Map<string, string | null>>(new Map())
  const [cacheVersion, setCacheVersion] = useState(0)

  const parsed = useMemo(
    () => (streaming ? extractPartialEditArgs(streaming.argsSnapshot) : null),
    [streaming],
  )

  const needsBaseFetch =
    !!streaming &&
    !!parsed?.path &&
    parsed.oldString === null &&
    parsed.content !== null &&
    !!workDir

  useEffect(() => {
    if (!needsBaseFetch || !parsed?.path) return
    const cacheKey = `${sessionId}:${parsed.path}`
    if (baseContentCache.current.has(cacheKey)) return
    baseContentCache.current.set(cacheKey, null)
    let cancelled = false
    workspaceFilesApi
      .readFile({ root: workDir, path: parsed.path })
      .then((file) => {
        if (cancelled) return
        if (file.isBinary || file.encoding !== 'utf8') {
          baseContentCache.current.set(cacheKey, null)
        } else {
          baseContentCache.current.set(cacheKey, file.content)
        }
        setCacheVersion((v) => v + 1)
      })
      .catch(() => {
        if (cancelled) return
        baseContentCache.current.set(cacheKey, '')
        setCacheVersion((v) => v + 1)
      })
    return () => {
      cancelled = true
    }
  }, [needsBaseFetch, parsed?.path, sessionId, workDir])

  if (!streaming || chatState === 'idle') return null

  const fileName = parsed?.path?.split(/[\\/]/).pop() ?? null

  let diffOld: string | null = null
  let diffNew: string | null = null
  if (parsed?.oldString !== null && parsed?.oldString !== undefined) {
    diffOld = parsed.oldString
    diffNew = parsed.newString ?? ''
  } else if (parsed?.content && parsed.path) {
    void cacheVersion
    const cached = baseContentCache.current.get(`${sessionId}:${parsed.path}`)
    if (cached !== null && cached !== undefined) {
      diffOld = cached
      diffNew = parsed.content
    }
  }

  const tail =
    diffOld === null && parsed?.content
      ? parsed.content.split('\n').slice(-PREVIEW_TAIL_LINES).join('\n')
      : null

  return (
    <div className="mx-auto w-full max-w-3xl px-4 pb-1">
      <div className="overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]/90">
        <div className="flex items-center gap-2 border-b border-[var(--color-border)]/40 px-3 py-1.5">
          <span className="material-symbols-outlined animate-pulse text-[14px] text-[var(--color-accent)]">
            edit_note
          </span>
          <span className="text-[11px] font-semibold text-[var(--color-text-secondary)]">
            {t('streamingEdit.writing')}
          </span>
          {fileName && (
            <span
              className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-primary)]"
              title={parsed?.path ?? undefined}
            >
              {fileName}
            </span>
          )}
          <span className="ml-auto font-[var(--font-mono)] text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
            {streaming.argsSnapshot.length.toLocaleString()} B
          </span>
        </div>
        {diffOld !== null && diffNew !== null ? (
          <div className="max-h-[240px] overflow-y-auto">
            <DiffViewer
              filePath={parsed?.path ?? ''}
              oldString={diffOld}
              newString={diffNew}
            />
          </div>
        ) : (
          tail && (
            <pre className="max-h-[180px] overflow-hidden whitespace-pre-wrap break-all px-3 py-1.5 font-[var(--font-mono)] text-[10.5px] leading-snug text-[var(--color-text-secondary)]">
              {tail}
            </pre>
          )
        )}
      </div>
    </div>
  )
}
