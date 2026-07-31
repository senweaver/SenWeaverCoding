// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { memo, useMemo } from 'react'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { StreamingMarkdownRenderer } from '../markdown/StreamingMarkdownRenderer'
import { AssistantMessageActions } from './AssistantMessageActions'
import { InlineImageGallery } from './InlineImageGallery'
import { sanitizeNarration } from '../../utils/sanitizeNarration'

type Props = {
  content: string
  isStreaming?: boolean

  assistantTurnCopyText?: string
  sessionId?: string | null
  workDir?: string | null
  disableFork?: boolean
}

function splitStreamingCommit(content: string): { committed: string; tail: string } {
  let lastBoundary = 0
  const fenceRe = /```/g
  const blankRe = /\n[ \t]*\n/g
  const fencePositions: number[] = []
  let fenceMatch: RegExpExecArray | null
  while ((fenceMatch = fenceRe.exec(content)) !== null) {
    fencePositions.push(fenceMatch.index)
  }
  let fenceIdx = 0
  let blankMatch: RegExpExecArray | null
  while ((blankMatch = blankRe.exec(content)) !== null) {
    const pos = blankMatch.index + blankMatch[0].length
    while (fenceIdx < fencePositions.length && (fencePositions[fenceIdx] ?? Infinity) < pos) {
      fenceIdx += 1
    }
    if (fenceIdx % 2 === 0) {
      lastBoundary = pos
    }
  }
  if (lastBoundary <= 0) {
    return { committed: '', tail: content }
  }
  return {
    committed: content.slice(0, lastBoundary),
    tail: content.slice(lastBoundary),
  }
}

export const AssistantMessage = memo(function AssistantMessage({
  content,
  isStreaming,
  assistantTurnCopyText,
  sessionId,
  workDir,
  disableFork,
}: Props) {
  const safeContent = useMemo(() => sanitizeNarration(content), [content])
  const documentLayout = useMemo(() => shouldUseDocumentLayout(safeContent), [safeContent])
  const streamingSplit = useMemo(
    () => (isStreaming ? splitStreamingCommit(safeContent) : null),
    [isStreaming, safeContent],
  )
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
            {isStreaming ? (
              <>
                {streamingSplit && streamingSplit.committed && (
                  <MarkdownRenderer
                    content={streamingSplit.committed}
                    variant={documentLayout ? 'document' : 'default'}
                    scale="chat"
                    streaming
                  />
                )}
                <StreamingMarkdownRenderer
                  content={streamingSplit ? streamingSplit.tail : safeContent}
                />
              </>
            ) : (
              <MarkdownRenderer
                content={safeContent}
                variant={documentLayout ? 'document' : 'default'}
                scale="chat"
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
