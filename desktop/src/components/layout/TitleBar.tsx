// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import {
  forwardRef,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
  type Ref,
} from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from '../../i18n'
import { useSessionStore } from '../../stores/sessionStore'
import { useTabStore } from '../../stores/tabStore'
import { focusSession } from '../../lib/focusSession'
import { useUIStore } from '../../stores/uiStore'
import { useUpdateStore } from '../../stores/updateStore'

const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
const isMacOSEnv =
  typeof navigator !== 'undefined' &&
  /Mac/i.test(navigator.platform) &&
  !(navigator.userAgent ?? '').includes('Mobile')

const ZOOM_STORAGE_KEY = 'senweaver-ui-zoom-pct'
const ZOOM_MIN = 75
const ZOOM_MAX = 150
const ZOOM_STEP = 12.5

const DOCS_URL = 'https://github.com/senweaver/SenWeaverCoding/blob/main/README.md'
const GITHUB_REPO_URL = 'https://github.com/senweaver/SenWeaverCoding'

function readZoomPct(): number {
  try {
    const raw = localStorage.getItem(ZOOM_STORAGE_KEY)
    const n = raw ? parseFloat(raw) : 100
    if (!Number.isFinite(n)) return 100
    return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, n))
  } catch {
    return 100
  }
}

function applyZoomPct(pct: number): void {
  const clamped = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, pct))
  try {
    localStorage.setItem(ZOOM_STORAGE_KEY, String(clamped))
  } catch {

  }
  document.documentElement.style.fontSize = `${(16 * clamped) / 100}px`
}

export function TitleBar() {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)
  const sidebarOpen = useUIStore((s) => s.sidebarOpen)
  const rightSidebarOpen = useUIStore((s) => s.rightSidebarOpen)
  const toggleSidebar = useUIStore((s) => s.toggleSidebar)
  const toggleRightSidebar = useUIStore((s) => s.toggleRightSidebar)

  type MenuId = 'file' | 'edit' | 'view' | 'help'
  const [openMenu, setOpenMenu] = useState<MenuId | null>(null)

  const fileBtnRef = useRef<HTMLButtonElement>(null)
  const editBtnRef = useRef<HTMLButtonElement>(null)
  const viewBtnRef = useRef<HTMLButtonElement>(null)
  const helpBtnRef = useRef<HTMLButtonElement>(null)

  const filePanelRef = useRef<HTMLDivElement>(null)
  const editPanelRef = useRef<HTMLDivElement>(null)
  const viewPanelRef = useRef<HTMLDivElement>(null)
  const helpPanelRef = useRef<HTMLDivElement>(null)

  const showMacTrafficLights = isTauri && isMacOSEnv
  const showWindowsCaption = isTauri && !isMacOSEnv

  const [captionMaximized, setCaptionMaximized] = useState(false)

  useEffect(() => {
    applyZoomPct(readZoomPct())
  }, [])

  useEffect(() => {
    if (!showWindowsCaption) return
    let cancelled = false
    let unlisten: (() => void) | undefined
    void import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
      if (cancelled) return
      const w = getCurrentWindow()
      const sync = async () => {
        if (!cancelled) setCaptionMaximized(await w.isMaximized())
      }
      await sync()
      const fn = await w.onResized(() => {
        void sync()
      })
      if (!cancelled) unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [showWindowsCaption])

  const closeAll = useCallback(() => setOpenMenu(null), [])

  useEffect(() => {
    if (!openMenu) return
    const onPointerDown = (e: MouseEvent) => {
      const node = e.target as Node
      const panels = [filePanelRef, editPanelRef, viewPanelRef, helpPanelRef]
      const triggers = [fileBtnRef, editBtnRef, viewBtnRef, helpBtnRef]
      for (const p of panels) {
        if (p.current?.contains(node)) return
      }
      for (const r of triggers) {
        if (r.current?.contains(node)) return
      }
      setOpenMenu(null)
    }
    document.addEventListener('mousedown', onPointerDown)
    return () => document.removeEventListener('mousedown', onPointerDown)
  }, [openMenu])

  const toggle = (id: MenuId) => {
    setOpenMenu((prev) => (prev === id ? null : id))
  }

  const openSettings = () => {
    closeAll()
    useUIStore.getState().toggleSettingsOverlay()
  }

  const newSession = async () => {
    closeAll()
    try {
      const currentTabId = useTabStore.getState().activeTabId
      const workDir = useSessionStore.getState().resolveWorkDirForNewSessionTab(currentTabId)
      const sessionId = await useSessionStore.getState().createSession(workDir)
      useTabStore.getState().openTab(sessionId, t('menu.file.newSession'))
      focusSession(sessionId)
    } catch (error) {
      addToast({
        type: 'error',
        message: error instanceof Error ? error.message : t('sidebar.sessionListFailed'),
      })
    }
  }

  const openWorkspaceFolder = async () => {
    closeAll()
    if (!isTauri) {
      addToast({
        type: 'error',
        message: t('menu.file.workspaceUnavailable'),
      })
      return
    }
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('menu.file.openWorkspaceFolder'),
      })
      const path = typeof selected === 'string' ? selected : Array.isArray(selected) ? selected[0] : null
      if (!path) return
      useSessionStore.getState().setUserPinnedSessionWorkDir(path)
      const sessionId = await useSessionStore.getState().createSession(path)
      useTabStore.getState().openTab(sessionId, t('menu.file.newSession'))
      focusSession(sessionId)
    } catch (error) {
      addToast({
        type: 'error',
        message: error instanceof Error ? error.message : t('sidebar.sessionListFailed'),
      })
    }
  }

  const quitApp = async () => {
    closeAll()
    if (!isTauri) return
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await getCurrentWindow().close()
    } catch {

    }
  }

  const checkForUpdatesMenu = () => {
    closeAll()
    void useUpdateStore.getState().checkForUpdates({ silent: false })
  }

  const openExternal = async (url: string) => {
    closeAll()
    try {
      const { open } = await import('@tauri-apps/plugin-shell')
      await open(url)
    } catch {
      window.open(url, '_blank', 'noopener,noreferrer')
    }
  }

  const zoomIn = () => {
    applyZoomPct(readZoomPct() + ZOOM_STEP)
    closeAll()
  }
  const zoomOut = () => {
    applyZoomPct(readZoomPct() - ZOOM_STEP)
    closeAll()
  }
  const zoomReset = () => {
    applyZoomPct(100)
    closeAll()
  }

  const undo = () => {
    closeAll()
    try {
      document.execCommand('undo')
    } catch {

    }
  }
  const redo = () => {
    closeAll()
    try {
      document.execCommand('redo')
    } catch {

    }
  }
  const cut = () => {
    closeAll()
    try {
      document.execCommand('cut')
    } catch {

    }
  }
  const copy = () => {
    closeAll()
    try {
      document.execCommand('copy')
    } catch {

    }
  }
  const paste = () => {
    closeAll()
    try {
      document.execCommand('paste')
    } catch {

    }
  }
  const selectAll = () => {
    closeAll()
    try {
      document.execCommand('selectAll')
    } catch {
      const ae = document.activeElement
      if (ae instanceof HTMLInputElement || ae instanceof HTMLTextAreaElement) {
        ae.select()
      }
    }
  }

  return (
    <div
      data-app-titlebar
      className="flex h-[var(--titlebar-height)] w-full shrink-0 select-none border-b border-[var(--color-border)] bg-[var(--color-surface)]"
    >
      {showMacTrafficLights ? (
        <MacTrafficLightsStrip t={t} />
      ) : (
        <div className="w-2 shrink-0" aria-hidden="true" />
      )}

      <div className="flex min-h-0 flex-1 items-center gap-1.5 px-2" data-tauri-drag-region>
        <img src="/app-icon.png" alt="" className="h-4 w-4 shrink-0" draggable={false} data-tauri-drag-region />

        <MenuTrigger ref={fileBtnRef} label={t('menu.file')} isOpen={openMenu === 'file'} onToggle={() => toggle('file')} />
        <MenuTrigger ref={editBtnRef} label={t('menu.edit')} isOpen={openMenu === 'edit'} onToggle={() => toggle('edit')} />
        <MenuTrigger ref={viewBtnRef} label={t('menu.view')} isOpen={openMenu === 'view'} onToggle={() => toggle('view')} />
        <MenuTrigger ref={helpBtnRef} label={t('menu.help')} isOpen={openMenu === 'help'} onToggle={() => toggle('help')} />

        <div className="min-w-8 flex-1" data-tauri-drag-region aria-hidden="true" />
      </div>

      {showWindowsCaption && <WindowsCaptionButtons isMaximized={captionMaximized} t={t} />}

      <AnchoredDropdown anchorRef={fileBtnRef} panelRef={filePanelRef} open={openMenu === 'file'}>
        <MenuRow onClick={() => void newSession()}>{t('menu.file.newSession')}</MenuRow>
        <MenuRow onClick={() => void openWorkspaceFolder()}>{t('menu.file.openWorkspaceFolder')}</MenuRow>
        <MenuDivider />
        <MenuRow onClick={openSettings}>{t('menu.file.settings')}</MenuRow>
        <MenuDivider />
        <MenuRow onClick={() => void quitApp()}>{t('menu.file.quit')}</MenuRow>
      </AnchoredDropdown>

      <AnchoredDropdown anchorRef={editBtnRef} panelRef={editPanelRef} open={openMenu === 'edit'}>
        <MenuRow onClick={undo}>{t('menu.edit.undo')}</MenuRow>
        <MenuRow onClick={redo}>{t('menu.edit.redo')}</MenuRow>
        <MenuDivider />
        <MenuRow onClick={cut}>{t('menu.edit.cut')}</MenuRow>
        <MenuRow onClick={copy}>{t('menu.edit.copy')}</MenuRow>
        <MenuRow onClick={paste}>{t('menu.edit.paste')}</MenuRow>
        <MenuRow onClick={selectAll}>{t('menu.edit.selectAll')}</MenuRow>
      </AnchoredDropdown>

      <AnchoredDropdown anchorRef={viewBtnRef} panelRef={viewPanelRef} open={openMenu === 'view'}>
        <MenuRow onClick={() => { toggleSidebar(); closeAll() }}>
          {sidebarOpen ? t('menu.view.hideLeftSidebar') : t('menu.view.showLeftSidebar')}
        </MenuRow>
        <MenuRow onClick={() => { toggleRightSidebar(); closeAll() }}>
          {rightSidebarOpen ? t('menu.view.hideRightSidebar') : t('menu.view.showRightSidebar')}
        </MenuRow>
        <MenuDivider />
        <MenuRow onClick={zoomIn}>{t('menu.view.zoomIn')}</MenuRow>
        <MenuRow onClick={zoomOut}>{t('menu.view.zoomOut')}</MenuRow>
        <MenuRow onClick={zoomReset}>{t('menu.view.resetZoom')}</MenuRow>
      </AnchoredDropdown>

      <AnchoredDropdown anchorRef={helpBtnRef} panelRef={helpPanelRef} open={openMenu === 'help'}>
        <MenuRow onClick={() => void openExternal(DOCS_URL)}>{t('menu.help.documentation')}</MenuRow>
        <MenuRow onClick={() => void openExternal(GITHUB_REPO_URL)}>{t('menu.help.github')}</MenuRow>
        <MenuDivider />
        <MenuRow onClick={checkForUpdatesMenu}>{t('menu.help.checkForUpdates')}</MenuRow>
      </AnchoredDropdown>
    </div>
  )
}

const MenuTrigger = forwardRef<
  HTMLButtonElement,
  { label: string; isOpen: boolean; onToggle: () => void }
>(function MenuTrigger({ label, isOpen, onToggle }, ref) {
  return (
    <button
      ref={ref}
      type="button"
      data-mac-menu-trigger=""
      aria-expanded={isOpen}
      aria-haspopup="menu"
      onClick={(e) => {
        e.stopPropagation()
        onToggle()
      }}
      className="rounded-[var(--radius-md)] bg-transparent px-1.5 py-0.5 text-[12px] leading-tight text-[var(--color-text-secondary)] shadow-none transition-colors hover:bg-transparent hover:text-[var(--color-text-primary)]"
    >
      {label}
    </button>
  )
})

function AnchoredDropdown({
  anchorRef,
  panelRef,
  open,
  children,
}: {
  anchorRef: RefObject<HTMLButtonElement | null>
  panelRef: RefObject<HTMLDivElement | null>
  open: boolean
  children: ReactNode
}) {
  const [pos, setPos] = useState({ top: 0, left: 0 })

  const updatePos = useCallback(() => {
    const el = anchorRef.current
    if (!el) return
    const r = el.getBoundingClientRect()
    setPos({ top: r.bottom + 2, left: r.left })
  }, [anchorRef])

  useLayoutEffect(() => {
    if (!open) return
    updatePos()
  }, [open, updatePos])

  useEffect(() => {
    if (!open) return
    window.addEventListener('scroll', updatePos, true)
    window.addEventListener('resize', updatePos)
    return () => {
      window.removeEventListener('scroll', updatePos, true)
      window.removeEventListener('resize', updatePos)
    }
  }, [open, updatePos])

  if (!open || typeof document === 'undefined') return null

  return createPortal(
    <div
      ref={panelRef as Ref<HTMLDivElement>}
      role="menu"
      data-mac-menu-panel=""
      className="fixed z-[230] min-w-[200px] rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-[var(--shadow-dropdown)]"
      style={{ top: pos.top, left: pos.left }}
    >
      {children}
    </div>,
    document.body,
  )
}

function MenuDivider() {
  return <div role="separator" className="my-1 h-px bg-[var(--color-border)]" />
}

function MenuRow({ children, onClick }: { children: ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className="flex w-full px-3 py-1.5 text-left text-xs text-[var(--color-text-primary)] transition-colors hover:bg-[var(--color-surface-hover)]"
    >
      {children}
    </button>
  )
}

async function tauriCaptionAction(kind: 'minimize' | 'toggle-maximize' | 'close'): Promise<void> {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  const w = getCurrentWindow()
  if (kind === 'minimize') await w.minimize()
  else if (kind === 'toggle-maximize') await w.toggleMaximize()
  else await w.close()
}

type T = ReturnType<typeof useTranslation>

function MacTrafficLightsStrip({ t }: { t: T }) {
  const closeLabel = t('titlebar.window.close')
  const minimizeLabel = t('titlebar.window.minimize')
  const zoomLabel = t('titlebar.window.zoom')
  return (
    <div className="flex h-full w-[78px] shrink-0 items-center gap-2 px-3 pt-px">
      <button
        type="button"
        title={closeLabel}
        aria-label={closeLabel}
        onClick={() => void tauriCaptionAction('close')}
        className="h-[11px] w-[11px] shrink-0 rounded-full bg-[#ff5f57] ring-1 ring-black/[0.12] hover:brightness-95 dark:ring-black/35"
      />
      <button
        type="button"
        title={minimizeLabel}
        aria-label={minimizeLabel}
        onClick={() => void tauriCaptionAction('minimize')}
        className="h-[11px] w-[11px] shrink-0 rounded-full bg-[#febc2f] ring-1 ring-black/[0.12] hover:brightness-95 dark:ring-black/35"
      />
      <button
        type="button"
        title={zoomLabel}
        aria-label={zoomLabel}
        onClick={() => void tauriCaptionAction('toggle-maximize')}
        className="h-[11px] w-[11px] shrink-0 rounded-full bg-[#28c840] ring-1 ring-black/[0.12] hover:brightness-95 dark:ring-black/35"
      />
    </div>
  )
}

function WindowsCaptionButtons({ isMaximized, t }: { isMaximized: boolean; t: T }) {
  const minimizeLabel = t('titlebar.window.minimize')
  const maximizeLabel = isMaximized
    ? t('titlebar.window.restore')
    : t('titlebar.window.maximize')
  const closeLabel = t('titlebar.window.close')
  return (
    <div className="flex h-full shrink-0">
      <button
        type="button"
        title={minimizeLabel}
        aria-label={minimizeLabel}
        onClick={() => void tauriCaptionAction('minimize')}
        className="inline-flex h-full w-[46px] items-center justify-center text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
      >
        <svg width="11" height="11" viewBox="0 0 11 11" fill="none" aria-hidden="true">
          <rect x="0" y="5" width="11" height="1" rx="0.5" fill="currentColor" />
        </svg>
      </button>
      <button
        type="button"
        title={maximizeLabel}
        aria-label={maximizeLabel}
        onClick={() => void tauriCaptionAction('toggle-maximize')}
        className="inline-flex h-full w-[46px] items-center justify-center text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
      >
        {isMaximized ? (
          <svg width="11" height="11" viewBox="0 0 11 11" fill="none" aria-hidden="true">
            <rect x="0" y="3" width="7" height="7" rx="1" stroke="currentColor" strokeWidth="1" />
            <rect x="3" y="1" width="7" height="7" rx="1" stroke="currentColor" strokeWidth="1" />
          </svg>
        ) : (
          <svg width="11" height="11" viewBox="0 0 11 11" fill="none" aria-hidden="true">
            <rect x="1" y="1" width="9" height="9" rx="1" stroke="currentColor" strokeWidth="1" />
          </svg>
        )}
      </button>
      <button
        type="button"
        title={closeLabel}
        aria-label={closeLabel}
        onClick={() => void tauriCaptionAction('close')}
        className="inline-flex h-full w-[46px] items-center justify-center text-[var(--color-text-secondary)] transition-colors hover:bg-[#e81123] hover:text-white"
      >
        <svg width="11" height="11" viewBox="0 0 11 11" fill="none" aria-hidden="true">
          <path
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1"
            d="M1.5 1.5 L9.5 9.5 M9.5 1.5 L1.5 9.5"
          />
        </svg>
      </button>
    </div>
  )
}
