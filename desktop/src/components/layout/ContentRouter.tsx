// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useTabStore } from '../../stores/tabStore'
import { EmptySession } from '../../pages/EmptySession'
import { ActiveSession } from '../../pages/ActiveSession'
import { Automations } from '../../pages/Automations'
import { WorkerSession } from '../../pages/WorkerSession'

export function ContentRouter() {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const activeTabType = useTabStore((s) => s.tabs.find((t) => t.sessionId === s.activeTabId)?.type)

  if (!activeTabId || !activeTabType) {
    return <EmptySession />
  }

  if (activeTabType === 'scheduled') {
    return <Automations />
  }

  if (activeTabType === 'worker') {
    return <WorkerSession workerId={activeTabId} />
  }

  return <ActiveSession />
}
