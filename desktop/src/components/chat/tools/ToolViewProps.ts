// SPDX-License-Identifier: MIT
//
// Shared prop types and helper utilities for the per-category tool views.
// All `*View` components under `desktop/src/components/chat/tools/` accept
// the same shape, so `ToolCard` can pick a renderer purely from the
// tool name.

import type { TranslationKey } from '../../../i18n'
import type { UIMessage } from '../../../types/chat'

export type ToolUseMessage = Extract<UIMessage, { type: 'tool_use' }>
export type ToolResultMessage = Extract<UIMessage, { type: 'tool_result' }>

export type ToolViewProps = {
  toolName: string
  toolUseId: string
  input: unknown
  result?: { content: unknown; isError: boolean } | null

  isStreaming?: boolean

  compact?: boolean

  childCalls?: ToolUseMessage[]

  childResults?: Map<string, ToolResultMessage>
}

export type Translator = (
  key: TranslationKey,
  params?: Record<string, string | number>,
) => string
