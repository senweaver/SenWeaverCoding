// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { isTauriRuntime } from './desktopRuntime'

/**
 * Ask the user to confirm a destructive action.
 *
 * In the Tauri desktop shell the native `window.confirm` is overridden to an async,
 * permission-gated plugin call, so a synchronous `if (window.confirm(...))` both throws
 * (when the permission is missing) and evaluates a Promise as truthy. This helper routes
 * through the dialog plugin in Tauri and falls back to `window.confirm` in a browser.
 */
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
