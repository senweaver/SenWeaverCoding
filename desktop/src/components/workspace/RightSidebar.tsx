import { useTabStore } from '../../stores/tabStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'
import { RightSidebarShell } from './RightSidebarShell'

export function RightSidebar() {
  const t = useTranslation()
  const open = useUIStore((s) => s.rightSidebarOpen)
  const width = useUIStore((s) => s.rightSidebarWidth)
  const setOpen = useUIStore((s) => s.setRightSidebarOpen)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessions = useSessionStore((s) => s.sessions)

  if (!open) return null

  const activeSession = activeTabId ? sessions.find((entry) => entry.id === activeTabId) : null
  const workDir = activeSession?.workDir ?? null

  return (
    <aside
      data-testid="right-sidebar"
      style={{ width }}
      className="flex flex-col flex-shrink-0 border-l border-[var(--color-border)] bg-[var(--color-surface)] min-h-0 h-full overflow-hidden"
    >
      <RightSidebarShell
        sessionId={activeTabId ?? null}
        workDir={workDir}
        onClose={() => setOpen(false)}
        emptyHint={t('rightSidebar.empty')}
      />
    </aside>
  )
}
