// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation, type TranslationKey } from '../../../i18n'
import { useMinimalStore } from '../../../stores/minimalStore'
import { useMinimalComputerStore, isComputerBusy } from '../../../stores/minimalComputerStore'
import { useMinimalRecorderStore } from '../../../stores/minimalRecorderStore'
import type { ComputerStatus } from '../../../stores/computerUseStore'
import {
  MINIMAL_COLLAPSED_HEIGHT,
  MINIMAL_COLLAPSED_WIDTH,
  MINIMAL_EVENT_COMPUTER_STOP,
  MINIMAL_EVENT_INPUT_HIDDEN,
  MINIMAL_EVENT_RECORDER_CONTROL,
  exitMinimalMode,
  hideMinimalInput,
  positionMinimalInitial,
  resizeMinimalWindow,
  showMinimalInput,
} from '../../../lib/minimalMode'
import { ComputerMenu } from './ComputerMenu'

const MENU_WIDTH = 220
const MENU_MIN_HEIGHT = 210
const STACK_GAP = 10
const DRAG_THRESHOLD = 4
const DOUBLE_CLICK_MS = 240
const INPUT_TOGGLE_WINDOW_MS = 400

function statusKey(status: ComputerStatus): TranslationKey {
  switch (status) {
    case 'connecting':
      return 'computerUse.connecting'
    case 'running':
      return 'computerUse.status.running'
    case 'thinking':
      return 'computerUse.status.thinking'
    case 'finished':
      return 'computerUse.status.finished'
    case 'call_user':
      return 'computerUse.status.callUser'
    case 'error':
      return 'computerUse.status.error'
    case 'stopped':
      return 'computerUse.status.stopped'
    default:
      return 'computerUse.status.idle'
  }
}

export function ComputerBar() {
  const t = useTranslation()
  const opacityPct = useMinimalStore((s) => s.opacityPct)

  const status = useMinimalComputerStore((s) => s.status)
  const statusMessage = useMinimalComputerStore((s) => s.statusMessage)
  const error = useMinimalComputerStore((s) => s.error)
  const lastThought = useMinimalComputerStore((s) => s.lastThought)
  const stepCount = useMinimalComputerStore((s) => s.stepCount)
  const pendingSteer = useMinimalComputerStore((s) => s.pendingSteer)
  const lastUserUpdate = useMinimalComputerStore((s) => s.lastUserUpdate)

  const recStatus = useMinimalRecorderStore((s) => s.status)
  const recStepCount = useMinimalRecorderStore((s) => s.stepCount)
  const recording = recStatus === 'recording'
  const generating = recStatus === 'generating'

  const busy = isComputerBusy(status)
  const attention = status === 'call_user'

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

  const stoppable = busy || attention || recording
  const handleOrbClick = (event: React.MouseEvent) => {
    if (dragStateRef.current?.dragging) return
    if (!stoppable) return
    event.stopPropagation()
    if (clickTimerRef.current !== null) {
      window.clearTimeout(clickTimerRef.current)
      clickTimerRef.current = null
    }
    void (async () => {
      try {
        const { emit } = await import('@tauri-apps/api/event')
        if (recording) {
          await emit(MINIMAL_EVENT_RECORDER_CONTROL, { action: 'stop' })
        } else {
          await emit(MINIMAL_EVENT_COMPUTER_STOP)
        }
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
      void showMinimalInput('computer')
    }, DOUBLE_CLICK_MS)
  }

  const handleContextMenu = (event: React.MouseEvent) => {
    event.preventDefault()
    setMenuOpen(true)
  }

  const recorderText = recording
    ? `${t('computerUse.record.recording')} · ${t('computerUse.record.stepsRecorded', {
        count: recStepCount,
      })}`
    : generating
      ? t('computerUse.record.generating')
      : null
  const steerText =
    busy && pendingSteer
      ? `${t('minimal.computer.steerPending')} · ${pendingSteer}`
      : busy && lastUserUpdate
        ? `${t('minimal.computer.steerApplied')} · ${lastUserUpdate}`
        : null
  const marqueeText =
    recorderText || error || steerText || lastThought || statusMessage || t(statusKey(status))
  const scrolling = busy || attention || recording || generating

  return (
    <div
      className="flex h-screen w-screen flex-col items-end justify-end gap-2.5 overflow-hidden p-4"
      style={{ opacity: opacityPct / 100 }}
    >
      {menuOpen && <ComputerMenu onClose={closeMenu} onHeightChange={setMenuHeight} />}

      <div
        role="button"
        tabIndex={0}
        aria-label={t('minimal.computer.subtitle')}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onClick={handleCardClick}
        onContextMenu={handleContextMenu}
        className="flex h-[64px] w-[168px] shrink-0 cursor-pointer select-none items-center gap-2.5 rounded-[20px] border border-white/60 px-2.5 shadow-[0_8px_28px_rgba(30,58,95,0.28)]"
        style={{
          background: recording
            ? 'linear-gradient(135deg, #ffd6d6 0%, #ffecec 45%, #cfe4ff 100%)'
            : attention
              ? 'linear-gradient(135deg, #ffe9c2 0%, #fff4e0 45%, #cfe4ff 100%)'
              : 'linear-gradient(135deg, #cfe4ff 0%, #e8f1ff 45%, #fdefc4 100%)',
        }}
      >
        <div
          role={stoppable ? 'button' : undefined}
          aria-label={
            stoppable
              ? recording
                ? t('computerUse.record.stop')
                : t('minimal.computer.stop')
              : undefined
          }
          title={
            stoppable
              ? recording
                ? t('computerUse.record.stop')
                : t('minimal.computer.stop')
              : undefined
          }
          onClick={handleOrbClick}
          className={`group/orb relative flex h-11 w-11 shrink-0 items-center justify-center ${
            stoppable ? 'cursor-pointer' : ''
          }`}
        >
          <span
            className={`absolute inset-0 rounded-full ${busy || recording ? 'animate-spin' : ''}`}
            style={{
              background: recording
                ? 'conic-gradient(from 180deg, #ef4444, #f87171, #fcd34d, #ef4444)'
                : 'conic-gradient(from 180deg, #60a5fa, #93c5fd, #fcd34d, #60a5fa)',
              opacity: busy || recording ? 1 : 0.55,
              animationDuration: '2.4s',
            }}
            aria-hidden
          />
          <span className="absolute inset-[3px] rounded-full bg-white/92" aria-hidden />
          <span className="relative flex flex-col items-center leading-none">
            {recording ? (
              <>
                <span className="text-[15px] font-bold text-[#b91c1c]">{recStepCount}</span>
                <span className="mt-[1px] text-[7px] font-medium uppercase tracking-wide text-[#b91c1c]">
                  {t('minimal.computer.rec')}
                </span>
              </>
            ) : busy ? (
              <>
                <span className="text-[15px] font-bold text-[#1e3a5f]">{stepCount}</span>
                <span className="mt-[1px] text-[7px] font-medium uppercase tracking-wide text-[#4a6b8a]">
                  {t('minimal.computer.steps')}
                </span>
              </>
            ) : (
              <span className="material-symbols-outlined text-[22px] text-[#1e3a5f]">
                desktop_windows
              </span>
            )}
          </span>
          {stoppable && (
            <span
              className="absolute inset-[3px] hidden items-center justify-center rounded-full bg-white/90 group-hover/orb:flex"
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
            <span className="truncate text-[9px] font-medium text-[#4a6b8a]">
              · {t('minimal.computer.subtitle')}
            </span>
          </div>
          <div className="mt-0.5 flex items-center gap-1">
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                recording || generating
                  ? 'bg-[#e5484d] animate-pulse-dot'
                  : attention
                    ? 'bg-[#e8850c] animate-pulse-dot'
                    : busy
                      ? 'bg-[#22a06b] animate-pulse-dot'
                      : 'bg-[#93a4b8]'
              }`}
              aria-hidden
            />
            <div className="relative min-w-0 flex-1 overflow-hidden">
              {scrolling ? (
                <div className="flex w-max animate-minimal-marquee">
                  <span className="whitespace-nowrap pr-8 text-[10px] text-[#4a5a72]">
                    {marqueeText}
                  </span>
                  <span
                    className="whitespace-nowrap pr-8 text-[10px] text-[#4a5a72]"
                    aria-hidden
                  >
                    {marqueeText}
                  </span>
                </div>
              ) : (
                <span className="block truncate text-[10px] text-[#4a5a72]">
                  {status === 'idle' ? t('minimal.computer.idle') : marqueeText}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
