// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { memo, useMemo, useRef } from 'react'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { StreamingMarkdownRenderer } from '../markdown/StreamingMarkdownRenderer'
import { AssistantMessageActions } from './AssistantMessageActions'
import { InlineImageGallery } from './InlineImageGallery'
import { sanitizeNarration } from '../../utils/sanitizeNarration'

type Props = {
  content: string
  isStreaming?: boolean

  assistantTurnCopyText?: string
  showThumbs?: boolean
  sessionId?: string | null
  workDir?: string | null
  disableFork?: boolean
}

type StreamingSplitState = {
  probeLen: number
  probe: string
  pos: number
  fenceParity: number
  boundary: number
}

function scanStreamingCommit(
  content: string,
  state: StreamingSplitState | null,
): StreamingSplitState {
  let pos = 0
  let fenceParity = 0
  let boundary = 0
  if (
    state &&
    content.length >= state.probeLen &&
    content.slice(state.probeLen - state.probe.length, state.probeLen) === state.probe
  ) {
    pos = state.pos
    fenceParity = state.fenceParity
    boundary = state.boundary
  }
  const n = content.length
  let i = pos
  while (i < n) {
    const c = content.charCodeAt(i)
    if (c === 96) {
      if (i + 2 < n) {
        if (content.charCodeAt(i + 1) === 96 && content.charCodeAt(i + 2) === 96) {
          fenceParity ^= 1
          i += 3
          continue
        }
        i += 1
        continue
      }
      break
    }
    if (c === 10) {
      let j = i + 1
      while (j < n) {
        const cj = content.charCodeAt(j)
        if (cj === 32 || cj === 9) {
          j += 1
          continue
        }
        break
      }
      if (j >= n) break
      if (content.charCodeAt(j) === 10) {
        if (fenceParity === 0) boundary = j + 1
        i = j + 1
        continue
      }
      i = j
      continue
    }
    i += 1
  }
  const probeStart = Math.max(0, i - 32)
  return {
    probeLen: i,
    probe: content.slice(probeStart, i),
    pos: i,
    fenceParity,
    boundary,
  }
}

export const AssistantMessage = memo(function AssistantMessage({
  content,
  isStreaming,
  assistantTurnCopyText,
  showThumbs,
  sessionId,
  workDir,
  disableFork,
}: Props) {
  const safeContent = useMemo(() => sanitizeNarration(content), [content])
  const documentLayout = useMemo(() => shouldUseDocumentLayout(safeContent), [safeContent])
  const splitStateRef = useRef<StreamingSplitState | null>(null)
  const streamingSplit = useMemo(() => {
    if (!isStreaming) {
      splitStateRef.current = null
      return null
    }
    const nextState = scanStreamingCommit(safeContent, splitStateRef.current)
    splitStateRef.current = nextState
    if (nextState.boundary <= 0) {
      return { committed: '', tail: safeContent }
    }
    return {
      committed: safeContent.slice(0, nextState.boundary),
      tail: safeContent.slice(nextState.boundary),
    }
  }, [isStreaming, safeContent])
  const showActions =
    !isStreaming && Boolean(assistantTurnCopyText?.trim())

  return (

    <div className="group mb-3 w-full">
      <div className="flex justify-start">
        <div
          data-message-shell="assistant"
          data-layout={documentLayout ? 'document' : 'inline'}
          className={`flex min-w-0 flex-col items-stretch gap-1 ${
            documentLayout
              ? 'w-full max-w-full'
              : 'w-full max-w-[88%] sm:max-w-[80%] lg:max-w-[72%]'
          }`}
        >
          <div className={`text-[var(--color-text-primary)] ${documentLayout ? 'w-full' : 'max-w-full'}`}>
            <MarkdownRenderer
              content={isStreaming ? streamingSplit?.committed ?? '' : safeContent}
              variant={documentLayout ? 'document' : 'default'}
              scale="chat"
              streaming={isStreaming}
            />
            {isStreaming && (
              <StreamingMarkdownRenderer
                content={streamingSplit ? streamingSplit.tail : safeContent}
              />
            )}
            {!isStreaming && <InlineImageGallery text={content} />}
            {}
          </div>
        </div>
      </div>

      {}
      {showActions && (
        <div className="mt-0.5 flex w-full justify-end pr-5">
          <AssistantMessageActions
            copyText={assistantTurnCopyText ?? ''}
            showThumbs={showThumbs}
            sessionId={sessionId}
            workDir={workDir}
            disableFork={disableFork}
          />
        </div>
      )}
    </div>
  )
})

function shouldUseDocumentLayout(content: string) {
  const normalized = content.trim()
  if (!normalized) return false

  if (/```/.test(normalized)) return true
  if (/^\s{0,3}(#{1,6}\s|[-*+]\s|\d+\.\s|>\s|\|.+\|)/m.test(normalized)) return true

  const paragraphs = normalized
    .split(/\n\s*\n/)
    .map((chunk) => chunk.trim())
    .filter(Boolean)

  return paragraphs.length >= 2 || normalized.split('\n').filter((line) => line.trim()).length >= 8
}
