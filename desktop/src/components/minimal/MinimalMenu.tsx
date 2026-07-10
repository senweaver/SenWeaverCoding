// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, type ReactNode } from 'react'
import { useTranslation } from '../../i18n'
import { useMinimalStore, MINIMAL_OPACITY_BOUNDS } from '../../stores/minimalStore'
import { forceQuit } from '../../lib/appClose'
import { exitMinimalMode, hideMinimalToTray, requestMainSettings } from '../../lib/minimalMode'

type MinimalMenuProps = {
  onClose: () => void
  onHeightChange: (height: number) => void
}

export function MinimalMenu({ onClose, onHeightChange }: MinimalMenuProps) {
  const t = useTranslation()
  const panelRef = useRef<HTMLDivElement>(null)
  const opacityPct = useMinimalStore((s) => s.opacityPct)
  const setOpacityPct = useMinimalStore((s) => s.setOpacityPct)

  useEffect(() => {
    const el = panelRef.current
    if (!el) return
    const report = () => {
      const h = Math.round(el.getBoundingClientRect().height)
      if (h > 0) onHeightChange(h)
    }
    report()
    const ro = new ResizeObserver(report)
    ro.observe(el)
    return () => ro.disconnect()
  }, [onHeightChange])

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      if (panelRef.current?.contains(event.target as Node)) return
      onClose()
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [onClose])

  return (
    <div
      ref={panelRef}
      role="menu"
      className="w-[188px] self-end rounded-xl border border-black/10 bg-white/95 py-1.5 shadow-[0_10px_40px_rgba(30,58,95,0.25)] backdrop-blur-md"
      onContextMenu={(e) => e.preventDefault()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <MinimalMenuRow
        icon="swap_horiz"
        label={t('minimal.menu.switch')}
        onClick={() => {
          onClose()
          void exitMinimalMode()
        }}
      />

      <div className="px-3 py-2">
        <div className="mb-1 flex items-center justify-between text-[11px] text-[#4a5a72]">
          <span className="inline-flex items-center gap-1.5">
            <span className="material-symbols-outlined text-[15px]">opacity</span>
            {t('minimal.menu.opacity')}
          </span>
          <span className="font-mono">{opacityPct}%</span>
        </div>
        <input
          type="range"
          min={MINIMAL_OPACITY_BOUNDS.min}
          max={MINIMAL_OPACITY_BOUNDS.max}
          step={5}
          value={opacityPct}
          onChange={(e) => setOpacityPct(Number.parseInt(e.target.value, 10))}
          className="h-1 w-full cursor-pointer accent-[#3b82f6]"
        />
      </div>

      <div className="my-1 h-px bg-black/8" />

      <MinimalMenuRow
        icon="settings"
        label={t('minimal.menu.settings')}
        onClick={() => {
          onClose()
          void requestMainSettings()
        }}
      />
      <MinimalMenuRow
        icon="dock_to_bottom"
        label={t('minimal.menu.tray')}
        onClick={() => {
          onClose()
          void hideMinimalToTray()
        }}
      />

      <div className="my-1 h-px bg-black/8" />

      <MinimalMenuRow
        icon="power_settings_new"
        label={t('minimal.menu.quit')}
        danger
        onClick={() => {
          onClose()
          void forceQuit()
        }}
      />
    </div>
  )
}

function MinimalMenuRow({
  icon,
  label,
  onClick,
  danger,
}: {
  icon: string
  label: ReactNode
  onClick: () => void
  danger?: boolean
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] transition-colors hover:bg-black/5 ${
        danger ? 'text-[#c0392b]' : 'text-[#22364f]'
      }`}
    >
      <span className="material-symbols-outlined text-[16px]">{icon}</span>
      {label}
    </button>
  )
}
