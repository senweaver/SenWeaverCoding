// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { StreamingMarkdownRenderer } from '../markdown/StreamingMarkdownRenderer'
import { AssistantMessageActions } from './AssistantMessageActions'
import { InlineImageGallery } from './InlineImageGallery'

type Props = {
  content: string
  isStreaming?: boolean

  assistantTurnCopyText?: string
  sessionId?: string | null
  workDir?: string | null
  disableFork?: boolean
}

export function AssistantMessage({
  content,
  isStreaming,
  assistantTurnCopyText,
  sessionId,
  workDir,
  disableFork,
}: Props) {
  const documentLayout = shouldUseDocumentLayout(content)
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
              <StreamingMarkdownRenderer content={content} />
            ) : (
              <MarkdownRenderer
                content={content}
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
            copyText={assistantTurnCopyText!}
            sessionId={sessionId}
            workDir={workDir}
            disableFork={disableFork}
          />
        </div>
      )}
    </div>
  )
}

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
