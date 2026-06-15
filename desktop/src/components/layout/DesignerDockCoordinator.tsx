// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect } from 'react'
import { useSettingsStore } from '../../stores/settingsStore'
import { useBrowserPanelStore } from '../../stores/browserPanelStore'
import { useTabStore } from '../../stores/tabStore'
import { dockHide } from '../../lib/browserDock'
import { isTauriRuntime } from '../../lib/desktopRuntime'

export function DesignerDockCoordinator() {
  const codingMode = useSettingsStore((s) => s.codingMode)
  const activeChatTabId = useTabStore((s) => s.activeTabId)
  const browserPanelVisible = useBrowserPanelStore((s) =>
    activeChatTabId ? s.panels[activeChatTabId]?.visible ?? false : false,
  )

  useEffect(() => {
    if (!isTauriRuntime()) return
    if (codingMode === 'designer' && !browserPanelVisible) {
      dockHide().catch((err) => {
        console.warn('[browserDock] dockHide for designer canvas failed', err)
      })
    }
  }, [codingMode, browserPanelVisible])

  return null
}
