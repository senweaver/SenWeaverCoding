// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useTabStore } from '../../stores/tabStore'
import { useSessionRunStateStore } from '../../stores/sessionRunStateStore'
import { useMinimalStore } from '../../stores/minimalStore'
import {
  MINIMAL_COLLAPSED_HEIGHT,
  MINIMAL_COLLAPSED_WIDTH,
  MINIMAL_EVENT_INPUT_HIDDEN,
  MINIMAL_EVENT_STOP,
  exitMinimalMode,
  hideMinimalInput,
  positionMinimalInitial,
  resizeMinimalWindow,
  showMinimalInput,
} from '../../lib/minimalMode'
import { MinimalMenu } from './MinimalMenu'

const MENU_WIDTH = 220
const MENU_MIN_HEIGHT = 210
const STACK_GAP = 10
const DRAG_THRESHOLD = 4
const DOUBLE_CLICK_MS = 240
const INPUT_TOGGLE_WINDOW_MS = 400

export function MinimalBar() {
  const t = useTranslation()
  const runningCount = useSessionRunStateStore((s) => s.running.size)
  const opacityPct = useMinimalStore((s) => s.opacityPct)

  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionTitle = useTabStore((s) =>
    activeTabId ? s.tabs.find((tab) => tab.sessionId === activeTabId)?.title ?? null : null,
  )
  const isRunning = useSessionRunStateStore((s) =>
    activeTabId ? s.running.has(activeTabId) : false,
  )

  const [menuOpen, setMenuOpen] = useState(false)
  const [menuHeight, setMenuHeight] = useState(0)

  const clickTimerRef = useRef<number | null>(null)
  const dragStateRef = useRef<{ x: number; y: number; dragging: boolean } | null>(null)
  const geometryReadyRef = useRef(false)
  const inputHiddenAtRef = useRef(0)

  useEffect(() => {
    void (async () => {
      await positionMinimalInitial()
      geometryReadyRef.current = true
    })()
    return () => {
      if (clickTimerRef.current !== null) window.clearTimeout(clickTimerRef.current)
    }
  }, [])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event')
        const off = await listen(MINIMAL_EVENT_INPUT_HIDDEN, () => {
          inputHiddenAtRef.current = Date.now()
        })
        if (disposed) off()
        else unlisten = off
      } catch {

      }
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])

  useEffect(() => {
    if (!geometryReadyRef.current) return
    if (!menuOpen) {
      void resizeMinimalWindow(MINIMAL_COLLAPSED_WIDTH, MINIMAL_COLLAPSED_HEIGHT)
      return
    }
    const extra = Math.max(menuHeight, MENU_MIN_HEIGHT) + STACK_GAP
    void resizeMinimalWindow(MENU_WIDTH, MINIMAL_COLLAPSED_HEIGHT + extra)
  }, [menuOpen, menuHeight])

  const closeMenu = useCallback(() => setMenuOpen(false), [])

  const handleAccelClick = (event: React.MouseEvent) => {
    if (dragStateRef.current?.dragging) return
    if (!isRunning || !activeTabId) return
    event.stopPropagation()
    if (clickTimerRef.current !== null) {
      window.clearTimeout(clickTimerRef.current)
      clickTimerRef.current = null
    }
    void (async () => {
      try {
        const { emit } = await import('@tauri-apps/api/event')
        await emit(MINIMAL_EVENT_STOP, activeTabId)
      } catch (err) {
        console.warn('[minimal] emit stop failed', err)
      }
    })()
  }

  useEffect(() => {
    if (!menuOpen) return
    let unlisten: (() => void) | null = null
    let cancelled = false
    void (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        const un = await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
          if (!focused) closeMenu()
        })
        if (cancelled) un()
        else unlisten = un
      } catch {

      }
    })()
    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
  }, [menuOpen, closeMenu])

  const handlePointerDown = (event: React.PointerEvent) => {
    if (event.button !== 0) return
    dragStateRef.current = { x: event.clientX, y: event.clientY, dragging: false }
  }

  const handlePointerMove = (event: React.PointerEvent) => {
    const state = dragStateRef.current
    if (!state || state.dragging) return
    if (
      Math.abs(event.clientX - state.x) > DRAG_THRESHOLD ||
      Math.abs(event.clientY - state.y) > DRAG_THRESHOLD
    ) {
      state.dragging = true
      void hideMinimalInput()
      void (async () => {
        try {
          const { getCurrentWindow } = await import('@tauri-apps/api/window')
          await getCurrentWindow().startDragging()
        } catch {

        }
      })()
    }
  }

  const handleCardClick = () => {
    if (dragStateRef.current?.dragging) {
      dragStateRef.current = null
      return
    }
    dragStateRef.current = null
    if (clickTimerRef.current !== null) {
      window.clearTimeout(clickTimerRef.current)
      clickTimerRef.current = null
      void exitMinimalMode()
      return
    }
    clickTimerRef.current = window.setTimeout(() => {
      clickTimerRef.current = null
      if (menuOpen) {
        setMenuOpen(false)
        return
      }
      if (Date.now() - inputHiddenAtRef.current < INPUT_TOGGLE_WINDOW_MS) return
      void showMinimalInput('code')
    }, DOUBLE_CLICK_MS)
  }

  const handleContextMenu = (event: React.MouseEvent) => {
    event.preventDefault()
    setMenuOpen(true)
  }

  const statusLabel = isRunning ? t('minimal.status.running') : t('minimal.status.idle')

  return (
    <div
      className="flex h-screen w-screen flex-col items-end justify-end gap-2.5 overflow-hidden p-4"
      style={{ opacity: opacityPct / 100 }}
    >
      {menuOpen && <MinimalMenu onClose={closeMenu} onHeightChange={setMenuHeight} />}

      <div
        role="button"
        tabIndex={0}
        aria-label={t('minimal.brand')}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onClick={handleCardClick}
        onContextMenu={handleContextMenu}
        className="flex h-[64px] w-[168px] shrink-0 cursor-pointer select-none items-center gap-2.5 rounded-[20px] border border-white/60 px-2.5 shadow-[0_8px_28px_rgba(30,58,95,0.28)]"
        style={{
          background:
            'linear-gradient(135deg, #cfe4ff 0%, #e8f1ff 45%, #fdefc4 100%)',
        }}
      >
        <div
          role={isRunning ? 'button' : undefined}
          aria-label={isRunning ? t('minimal.stop') : undefined}
          title={isRunning ? t('minimal.stop') : undefined}
          onClick={handleAccelClick}
          className={`group/accel relative flex h-11 w-11 shrink-0 items-center justify-center ${
            isRunning ? 'cursor-pointer' : ''
          }`}
        >
          <span
            className="absolute inset-0 rounded-full"
            style={{
              background:
                'conic-gradient(from 180deg, #60a5fa, #93c5fd, #fcd34d, #60a5fa)',
              opacity: isRunning ? 1 : 0.55,
            }}
            aria-hidden
          />
          <span className="absolute inset-[3px] rounded-full bg-white/92" aria-hidden />
          <span className="relative flex flex-col items-center leading-none">
            <span className="text-[15px] font-bold text-[#1e3a5f]">{runningCount}</span>
            <span className="mt-[1px] text-[7px] font-medium uppercase tracking-wide text-[#4a6b8a]">
              {t('minimal.agents')}
            </span>
          </span>
          {isRunning && (
            <span
              className="absolute inset-[3px] hidden items-center justify-center rounded-full bg-white/90 group-hover/accel:flex"
              aria-hidden
            >
              <span className="h-3 w-3 rounded-[3px] bg-[#e5484d]" />
            </span>
          )}
        </div>

        <div className="flex min-w-0 flex-1 flex-col justify-center">
          <div className="flex items-center gap-1">
            <img src="/app-icon.svg" alt="" className="h-3.5 w-3.5 shrink-0" draggable={false} />
            <span className="truncate text-[12px] font-semibold text-[#1e3a5f]">
              {t('minimal.brand')}
            </span>
          </div>
          <div className="mt-0.5 flex items-center gap-1">
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                isRunning ? 'bg-[#22a06b] animate-pulse-dot' : 'bg-[#93a4b8]'
              }`}
              aria-hidden
            />
            <span className="truncate text-[10px] text-[#4a5a72]">
              {sessionTitle ? `${statusLabel} · ${sessionTitle}` : statusLabel}
            </span>
          </div>
        </div>
      </div>
    </div>
  )
}
