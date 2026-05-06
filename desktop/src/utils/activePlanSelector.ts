

import type { ChatState, UIMessage } from '../types/chat'

export type PlanExecutionState =

  | 'idle'

  | 'pending_switch'

  | 'executing'

  | 'incomplete_run'

  | 'completed_run'

type PlanCardMsg = Extract<UIMessage, { type: 'plan_card' }>
type ModeSwitchMsg = Extract<UIMessage, { type: 'mode_switch_card' }>

function findFollowingSwitchCard(
  messages: UIMessage[],
  planCardIdx: number,
  card: PlanCardMsg,
): ModeSwitchMsg | null {
  let latest: ModeSwitchMsg | null = null
  for (let j = planCardIdx + 1; j < messages.length; j++) {
    const m = messages[j]
    if (!m) continue
    if (m.type === 'plan_card' && m.planPath !== card.planPath) {

      break
    }
    if (m.type === 'mode_switch_card' && m.planPath === card.planPath) {
      latest = m
    }
  }
  return latest
}

function allTodosTerminal(card: PlanCardMsg): boolean {
  if (card.todos.length === 0) return false
  return card.todos.every(
    (t) => t.status === 'completed' || t.status === 'cancelled',
  )
}

export function selectPlanCardExecutionState(
  messages: UIMessage[],
  planCardId: string,
  chatState?: ChatState,
): PlanExecutionState {
  const idx = messages.findIndex(
    (m) => m.id === planCardId && m.type === 'plan_card',
  )
  if (idx < 0) return 'idle'
  const card = messages[idx] as PlanCardMsg
  const switchCard = findFollowingSwitchCard(messages, idx, card)
  if (!switchCard) return 'idle'
  if (switchCard.status === 'pending') return 'pending_switch'
  if (switchCard.status === 'dismissed') return 'idle'

  if (allTodosTerminal(card)) return 'completed_run'

  if (chatState !== undefined && chatState === 'idle') {
    return 'incomplete_run'
  }
  return 'executing'
}

export function selectActiveExecutingPlan(
  messages: UIMessage[] | undefined,
  chatState?: ChatState,
): { card: PlanCardMsg; state: PlanExecutionState } | null {
  if (!messages) return null
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]
    if (m && m.type === 'plan_card') {
      const state = selectPlanCardExecutionState(messages, m.id, chatState)
      if (state === 'executing' || state === 'incomplete_run') {
        return { card: m, state }
      }

      return null
    }
  }
  return null
}
