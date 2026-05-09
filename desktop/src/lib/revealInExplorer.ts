// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { isTauriRuntime } from './desktopRuntime'

type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>

let invokeRef: InvokeFn | null = null
let bootPromise: Promise<void> | null = null

async function ensureBoot(): Promise<void> {
  if (!isTauriRuntime()) return
  if (invokeRef) return
  if (!bootPromise) {
    bootPromise = (async () => {
      const core = (await import(/* @vite-ignore */ '@tauri-apps/api/core')) as {
        invoke: InvokeFn
      }
      invokeRef = core.invoke
    })().catch((err) => {
      bootPromise = null
      throw err
    })
  }
  await bootPromise
}

export async function revealInExplorer(path: string): Promise<void> {
  if (!path || !path.trim()) {
    throw new Error('reveal: empty path')
  }
  if (!isTauriRuntime()) {
    throw new Error('reveal: not running inside the desktop runtime')
  }
  await ensureBoot()
  if (!invokeRef) {
    throw new Error('reveal: tauri ipc not available')
  }
  await invokeRef('reveal_in_explorer', { path })
}
