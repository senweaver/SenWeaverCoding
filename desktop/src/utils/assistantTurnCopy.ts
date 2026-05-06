import type { UIMessage } from '../types/chat'

export type AssistantTurnCopyInfo = {
  fullText: string
  isLastAssistantSegmentInTurn: boolean
}

export function buildAssistantTurnCopyMap(
  messages: UIMessage[],
): Map<string, AssistantTurnCopyInfo> {
  const out = new Map<string, AssistantTurnCopyInfo>()
  let segmentStart = 0
  while (segmentStart < messages.length) {
    let segmentEnd = messages.length
    for (let i = segmentStart; i < messages.length; i++) {
      const m = messages[i]
      if (m && m.type === 'user_text' && i > segmentStart) {
        segmentEnd = i
        break
      }
    }
    const turnIds: string[] = []
    let combined = ''
    for (let i = segmentStart; i < segmentEnd; i++) {
      const m = messages[i]
      if (m && m.type === 'assistant_text') {
        combined = combined ? `${combined}\n\n${m.content}` : m.content
        turnIds.push(m.id)
      }
    }
    if (turnIds.length > 0) {
      const lastId = turnIds[turnIds.length - 1]
      for (const id of turnIds) {
        out.set(id, {
          fullText: combined,
          isLastAssistantSegmentInTurn: id === lastId,
        })
      }
    }
    if (segmentEnd === messages.length) break
    segmentStart = segmentEnd
  }
  return out
}

export function getAssistantTurnCopyInfo(messages: UIMessage[], currentId: string): {
  fullText: string
  isLastAssistantSegmentInTurn: boolean
} | null {
  const idx = messages.findIndex((m) => m.id === currentId)
  if (idx < 0) return null
  const msg = messages[idx]
  if (!msg || msg.type !== 'assistant_text') return null

  let start = 0
  for (let i = idx - 1; i >= 0; i--) {
    const prev = messages[i]
    if (prev && prev.type === 'user_text') {
      start = i + 1
      break
    }
  }

  let end = messages.length
  for (let i = idx + 1; i < messages.length; i++) {
    const next = messages[i]
    if (next && next.type === 'user_text') {
      end = i
      break
    }
  }

  const parts: string[] = []
  let lastAssistantId: string | null = null
  for (let i = start; i < end; i++) {
    const m = messages[i]
    if (m && m.type === 'assistant_text') {
      parts.push(m.content)
      lastAssistantId = m.id
    }
  }

  return {
    fullText: parts.join('\n\n'),
    isLastAssistantSegmentInTurn: lastAssistantId === currentId,
  }
}
