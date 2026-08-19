// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type LanIdentity = {
  userId: string
  hostname: string
  nickname: string
  email: string | null
  localIp: string | null
  publicKey: string
  running: boolean
  port: number
  configuredEnabled?: boolean
}

export type LanPeer = {
  userId: string
  nickname: string
  publicKey: string
  ip: string
  port: number
  online: boolean
}

export type LanMessage = {
  id: string
  peerId: string
  direction: 'in' | 'out'
  kind: 'text' | 'file'
  body: string
  fileName?: string | null
  filePath?: string | null
  fileSize?: number | null
  createdAt: number
  read: boolean
}

export type LanConversation = {
  peerId: string
  nickname: string
  lastMessage: string
  lastTs: number
  unread: number
}

export type LanTransfer = {
  id: string
  peerId: string
  direction: 'in' | 'out'
  name: string
  path?: string | null
  size: number
  transferred: number
  status: 'active' | 'completed' | 'failed'
}

export type LanEvent =
  | { type: 'lan_event'; kind: 'lan_status'; data: { running: boolean; port: number } }
  | { type: 'lan_event'; kind: 'lan_identity'; data: LanIdentity }
  | { type: 'lan_event'; kind: 'lan_peers'; data: { peers: LanPeer[] } }
  | { type: 'lan_event'; kind: 'lan_message'; data: { message: LanMessage } }
  | { type: 'lan_event'; kind: 'lan_unread'; data: { unread: number } }
  | { type: 'lan_event'; kind: 'lan_transfer'; data: { transfer: LanTransfer } }
