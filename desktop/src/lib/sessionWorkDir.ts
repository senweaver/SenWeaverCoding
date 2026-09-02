// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useSessionStore } from '../stores/sessionStore'
import { useTabStore } from '../stores/tabStore'
import { useChatStore } from '../stores/chatStore'
import { useSettingsStore } from '../stores/settingsStore'
import { useSessionRuntimeStore } from '../stores/sessionRuntimeStore'

const replacedSessionIds = new Map<string, string>()
const inFlightSwitches = new Map<string, Promise<string>>()

export function resolveReplacedSessionId(sessionId: string): string {
  let current = sessionId
  const visited = new Set<string>()
  while (!visited.has(current)) {
    visited.add(current)
    const next = replacedSessionIds.get(current)
    if (!next) break
    current = next
  }
  return current
}

export async function resolveReplacedSessionIdSettled(sessionId: string): Promise<string> {
  let current = resolveReplacedSessionId(sessionId)
  const visited = new Set<string>()
  while (!visited.has(current)) {
    visited.add(current)
    const pending = inFlightSwitches.get(current)
    if (!pending) break
    try {
      current = resolveReplacedSessionId(await pending)
    } catch {
      break
    }
  }
  return current
}

export function switchEmptySessionWorkDir(
  sessionId: string,
  newWorkDir: string,
): Promise<string> {
  const oldId = resolveReplacedSessionId(sessionId)
  const pending = inFlightSwitches.get(oldId)
  if (pending) {
    return pending.then((settledId) => switchEmptySessionWorkDir(settledId, newWorkDir))
  }
  const task = performSwitch(oldId, newWorkDir)
  inFlightSwitches.set(oldId, task)
  void task
    .finally(() => {
      if (inFlightSwitches.get(oldId) === task) inFlightSwitches.delete(oldId)
    })
    .catch(() => {})
  return task
}

async function performSwitch(oldId: string, newWorkDir: string): Promise<string> {
  const sessionStore = useSessionStore.getState()
  sessionStore.setUserPinnedSessionWorkDir(newWorkDir)
  const newId = await sessionStore.createSession(newWorkDir)
  replacedSessionIds.set(oldId, newId)
  useSessionRuntimeStore.getState().moveSelection(oldId, newId)
  const chat = useChatStore.getState()
  chat.disconnectSession(oldId)
  useTabStore.getState().replaceTabSession(oldId, newId)
  chat.connectToSession(newId)
  chat.setSessionPermissionMode(newId, useSettingsStore.getState().permissionMode)
  useSessionStore
    .getState()
    .deleteSession(oldId)
    .catch(() => {})
  return newId
}
