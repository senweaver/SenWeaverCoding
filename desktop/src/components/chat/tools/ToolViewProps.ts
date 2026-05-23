// SPDX-License-Identifier: MIT

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

  parentSessionId?: string | null

  toolTimestamp?: number

  childCalls?: ToolUseMessage[]

  childResults?: Map<string, ToolResultMessage>
}

export type Translator = (
  key: TranslationKey,
  params?: Record<string, string | number>,
) => string
