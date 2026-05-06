import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'

export function RightSidebarToggle() {
  const t = useTranslation()
  const open = useUIStore((s) => s.rightSidebarOpen)
  const toggleRightSidebar = useUIStore((s) => s.toggleRightSidebar)

  return (
    <button
      type="button"
      data-testid="right-sidebar-toggle"
      onClick={toggleRightSidebar}
      aria-pressed={open}
      aria-label={open ? t('rightSidebar.toggleClose') : t('rightSidebar.toggleOpen')}
      title={open ? t('rightSidebar.toggleClose') : t('rightSidebar.toggleOpen')}
      className={`flex-shrink-0 w-9 h-[37px] flex items-center justify-center transition-colors ${
        open
          ? 'text-[var(--color-text-primary)] bg-[var(--color-surface-hover)]'
          : 'text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]'
      }`}
    >
      <span className="material-symbols-outlined text-[18px]">
        {open ? 'right_panel_close' : 'right_panel_open'}
      </span>
    </button>
  )
}
