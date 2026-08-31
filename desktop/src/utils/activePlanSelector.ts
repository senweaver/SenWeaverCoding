// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



import type { ChatState, UIMessage } from '../types/chat'
import { hasNewUserTurnAfter } from './activeCuratorSelector'

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
  if (!switchCard || switchCard.status === 'dismissed') {
    return 'idle'
  }
  if (switchCard.status === 'pending') return 'pending_switch'
  if (switchCard.status !== 'switched') return 'idle'

  if (allTodosTerminal(card)) return 'completed_run'

  if (hasNewUserTurnAfter(messages, switchCard.id)) return 'completed_run'

  if (chatState !== undefined && chatState === 'idle') {
    const hasTodos = card.todos.length > 0
    return hasTodos ? 'incomplete_run' : 'completed_run'
  }
  return 'executing'
}

const planExecStateCache = new WeakMap<UIMessage[], Map<string, PlanExecutionState>>()

export function selectPlanCardExecutionStateCached(
  messages: UIMessage[],
  planCardId: string,
  chatState?: ChatState,
): PlanExecutionState {
  const key = `${planCardId}:${chatState ?? '__undef__'}`
  let inner = planExecStateCache.get(messages)
  if (!inner) {
    inner = new Map()
    planExecStateCache.set(messages, inner)
  }
  const hit = inner.get(key)
  if (hit !== undefined) return hit
  const computed = selectPlanCardExecutionState(messages, planCardId, chatState)
  inner.set(key, computed)
  return computed
}

type ActivePlanResult = { card: PlanCardMsg; state: PlanExecutionState } | null

const activePlanCache = new WeakMap<UIMessage[], Map<string, ActivePlanResult>>()

function computeActiveExecutingPlan(
  messages: UIMessage[],
  chatState?: ChatState,
): ActivePlanResult {
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

export function selectActiveExecutingPlan(
  messages: UIMessage[] | undefined,
  chatState?: ChatState,
): ActivePlanResult {
  if (!messages) return null
  const key = chatState ?? '__undef__'
  let inner = activePlanCache.get(messages)
  if (inner) {
    const cached = inner.get(key)
    if (cached !== undefined) return cached
  } else {
    inner = new Map()
    activePlanCache.set(messages, inner)
  }
  const result = computeActiveExecutingPlan(messages, chatState)
  inner.set(key, result)
  return result
}
