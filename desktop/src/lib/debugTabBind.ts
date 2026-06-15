// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { wsManager } from '../api/websocket'
import { useBrowserPanelStore } from './../stores/browserPanelStore'

export function bindDebugTab(sessionId: string, tabId: number) {
  if (!sessionId || !Number.isFinite(tabId)) return
  wsManager.send(sessionId, { type: 'debug_bind_tab', tab_id: tabId })
  void useBrowserPanelStore.getState().setPreferredTestTab(sessionId, tabId)
}

export function unbindDebugTab(sessionId: string, tabId: number) {
  if (!sessionId || !Number.isFinite(tabId)) return
  wsManager.send(sessionId, { type: 'debug_unbind_tab', tab_id: tabId })
  void useBrowserPanelStore.getState().clearPreferredTestTab(sessionId)
}

export function bindPrototypeRef(sessionId: string, tabId: number) {
  if (!sessionId || !Number.isFinite(tabId)) return
  wsManager.send(sessionId, { type: 'debug_bind_prototype_ref', tab_id: tabId })
  useBrowserPanelStore.getState().setPrototypeRefTab(sessionId, tabId)
}

export function bindPrototypeFigma(sessionId: string, url: string) {
  const trimmed = url.trim()
  if (!sessionId || !trimmed.includes('figma.com/')) return
  wsManager.send(sessionId, { type: 'debug_bind_prototype_ref', figma_url: trimmed })
  useBrowserPanelStore.getState().setPrototypeRefFigma(sessionId, trimmed)
}

export function unbindPrototypeRef(sessionId: string) {
  if (!sessionId) return
  wsManager.send(sessionId, { type: 'debug_unbind_prototype_ref' })
  useBrowserPanelStore.getState().clearPrototypeRefTab(sessionId)
  useBrowserPanelStore.getState().clearPrototypeRefFigma(sessionId)
}
