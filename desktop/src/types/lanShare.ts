// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type LanMyShare = {
  id: string
  name: string
  path: string
  isDir: boolean
  size: number
  note: string
  createdAt: number
}

export type LanPeerShare = {
  id: string
  ownerId: string
  ownerNickname: string
  name: string
  isDir: boolean
  size: number
  note: string
  online: boolean
  createdAt: number
}

export type LanShareDownloaded = {
  peerId: string
  ownerNickname: string
  shareId: string
  name: string
  path: string
  isDir: boolean
  size: number
}
