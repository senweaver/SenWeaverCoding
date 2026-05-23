

import type { ChatState, UIMessage } from '../types/chat'

export type CuratorExecutionState =
  | 'idle'
  | 'pending_switch'
  | 'executing'
  | 'completed_run'

type CuratorCardMsg = Extract<UIMessage, { type: 'curator_card' }>
type ModeSwitchMsg = Extract<UIMessage, { type: 'mode_switch_card' }>

function findFollowingCuratorSwitchCard(
  messages: UIMessage[],
  curatorCardIdx: number,
  card: CuratorCardMsg,
): ModeSwitchMsg | null {
  const targetPath = card.implBlueprintPath || card.finalMdPath
  let latest: ModeSwitchMsg | null = null
  for (let j = curatorCardIdx + 1; j < messages.length; j++) {
    const m = messages[j]
    if (!m) continue
    if (m.type === 'curator_card' && m.id !== card.id) {
      break
    }
    if (
      m.type === 'mode_switch_card' &&
      m.handoffKind === 'curator' &&
      (m.planPath === targetPath || m.planPath === card.finalMdPath)
    ) {
      latest = m
    }
  }
  return latest
}

function hasNewUserTurnAfter(
  messages: UIMessage[],
  switchMessageId: string,
): boolean {
  const switchIdx = messages.findIndex((m) => m.id === switchMessageId)
  if (switchIdx < 0) return false
  for (let j = switchIdx + 1; j < messages.length; j++) {
    const m = messages[j]
    if (m && m.type === 'user_text') return true
  }
  return false
}

export function selectCuratorCardExecutionState(
  messages: UIMessage[],
  curatorCardId: string,
  chatState?: ChatState,
): CuratorExecutionState {
  const idx = messages.findIndex(
    (m) => m.id === curatorCardId && m.type === 'curator_card',
  )
  if (idx < 0) return 'idle'
  const card = messages[idx] as CuratorCardMsg
  const switchCard = findFollowingCuratorSwitchCard(messages, idx, card)
  if (!switchCard) return 'idle'
  if (switchCard.status === 'pending') return 'pending_switch'
  if (switchCard.status === 'dismissed') return 'idle'

  if (hasNewUserTurnAfter(messages, switchCard.id)) return 'completed_run'

  if (chatState !== undefined && chatState === 'idle') return 'completed_run'

  return 'executing'
}
