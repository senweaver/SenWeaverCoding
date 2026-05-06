import { useTabStore } from '../../stores/tabStore'
import { EmptySession } from '../../pages/EmptySession'
import { ActiveSession } from '../../pages/ActiveSession'
import { ScheduledTasks } from '../../pages/ScheduledTasks'

export function ContentRouter() {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const activeTabType = useTabStore((s) => s.tabs.find((t) => t.sessionId === s.activeTabId)?.type)

  if (!activeTabId || !activeTabType) {
    return <EmptySession />
  }

  if (activeTabType === 'scheduled') {
    return <ScheduledTasks />
  }

  return <ActiveSession />
}
