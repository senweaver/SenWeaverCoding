// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { isTauriRuntime } from './desktopRuntime'

export async function confirmDialog(message: string, title?: string): Promise<boolean> {
  if (isTauriRuntime()) {
    try {
      const { confirm } = await import('@tauri-apps/plugin-dialog')
      return await confirm(message, title ? { title } : undefined)
    } catch {
      return false
    }
  }
  try {
    return window.confirm(message)
  } catch {
    return false
  }
}
