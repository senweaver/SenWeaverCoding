// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



import type { ChatState, UIMessage } from '../types/chat'

export type CuratorExecutionState =
  | 'idle'
  | 'pending_switch'
  | 'executing'
  | 'incomplete_run'
  | 'completed_run'

type CuratorCardMsg = Extract<UIMessage, { type: 'curator_card' }>
type ModeSwitchMsg = Extract<UIMessage, { type: 'mode_switch_card' }>

function curatorTodosTerminal(card: CuratorCardMsg): boolean {
  const todos = card.todos ?? []
  if (todos.length === 0) return false
  return todos.every((t) => t.status === 'completed' || t.status === 'cancelled')
}

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

export function hasNewUserTurnAfter(
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

  if (card.wasExecuted && curatorTodosTerminal(card)) return 'completed_run'

  const switchCard = findFollowingCuratorSwitchCard(messages, idx, card)
  if (!switchCard) {
    if (card.wasExecuted) {
      return chatState === undefined || chatState === 'idle'
        ? 'completed_run'
        : 'executing'
    }
    return 'idle'
  }
  if (switchCard.status === 'pending') return 'pending_switch'
  if (switchCard.status === 'dismissed') return 'idle'

  if (curatorTodosTerminal(card)) return 'completed_run'

  if (hasNewUserTurnAfter(messages, switchCard.id)) return 'completed_run'

  if (chatState !== undefined && chatState === 'idle') {
    const hasTodos = (card.todos ?? []).length > 0
    return hasTodos ? 'incomplete_run' : 'completed_run'
  }

  return 'executing'
}

const curatorExecStateCache = new WeakMap<
  UIMessage[],
  Map<string, CuratorExecutionState>
>()

export function selectCuratorCardExecutionStateCached(
  messages: UIMessage[],
  curatorCardId: string,
  chatState?: ChatState,
): CuratorExecutionState {
  const key = `${curatorCardId}:${chatState ?? '__undef__'}`
  let inner = curatorExecStateCache.get(messages)
  if (!inner) {
    inner = new Map()
    curatorExecStateCache.set(messages, inner)
  }
  const hit = inner.get(key)
  if (hit !== undefined) return hit
  const computed = selectCuratorCardExecutionState(messages, curatorCardId, chatState)
  inner.set(key, computed)
  return computed
}

type ActiveCuratorResult = { card: CuratorCardMsg; state: CuratorExecutionState } | null

const activeCuratorCache = new WeakMap<UIMessage[], Map<string, ActiveCuratorResult>>()

function computeActiveExecutingCurator(
  messages: UIMessage[],
  chatState?: ChatState,
): ActiveCuratorResult {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]
    if (m && m.type === 'curator_card') {
      const state = selectCuratorCardExecutionState(messages, m.id, chatState)
      if (state === 'executing' || state === 'incomplete_run') {
        return { card: m, state }
      }
      return null
    }
  }
  return null
}

export function selectActiveExecutingCurator(
  messages: UIMessage[] | undefined,
  chatState?: ChatState,
): ActiveCuratorResult {
  if (!messages) return null
  const key = chatState ?? '__undef__'
  let inner = activeCuratorCache.get(messages)
  if (inner) {
    const cached = inner.get(key)
    if (cached !== undefined) return cached
  } else {
    inner = new Map()
    activeCuratorCache.set(messages, inner)
  }
  const result = computeActiveExecutingCurator(messages, chatState)
  inner.set(key, result)
  return result
}
