// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, useState } from 'react'
import { useTranslation, type TranslationKey } from '../../../i18n'
import { useMinimalRecorderStore } from '../../../stores/minimalRecorderStore'
import { ComputerComposer } from './ComputerComposer'
import { RecorderPane } from './RecorderPane'
import { SkillsPane } from './SkillsPane'

type PanelTab = 'task' | 'record' | 'skills'

type ComputerPanelProps = {
  onHeightChange: (height: number) => void
  onSubmitted: () => void
}

const TAB_KEYS: Record<PanelTab, TranslationKey> = {
  task: 'minimal.computer.tab.task',
  record: 'minimal.computer.tab.record',
  skills: 'minimal.computer.tab.skills',
}

const TAB_ICONS: Record<PanelTab, string> = {
  task: 'play_circle',
  record: 'fiber_manual_record',
  skills: 'auto_awesome_motion',
}

const noopHeight = () => {}

export function ComputerPanel({ onHeightChange, onSubmitted }: ComputerPanelProps) {
  const t = useTranslation()
  const wrapRef = useRef<HTMLDivElement>(null)

  const recStatus = useMinimalRecorderStore((s) => s.status)
  const recActive = recStatus !== 'idle'

  const [tab, setTab] = useState<PanelTab>(() =>
    useMinimalRecorderStore.getState().status !== 'idle' ? 'record' : 'task',
  )

  useEffect(() => {
    if (recStatus === 'recording') setTab('record')
  }, [recStatus])

  useEffect(() => {
    const el = wrapRef.current
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

  return (
    <div ref={wrapRef} className="flex flex-col gap-1.5">
      <div className="flex items-center gap-1 self-start rounded-full border border-white/50 bg-[var(--color-surface)]/95 p-0.5 shadow-[0_6px_24px_rgba(30,58,95,0.22)] backdrop-blur-md">
        {(Object.keys(TAB_KEYS) as PanelTab[]).map((key) => {
          const active = tab === key
          const highlight = key === 'record' && recActive
          return (
            <button
              key={key}
              type="button"
              onClick={() => setTab(key)}
              className={`inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[10.5px] font-medium transition-colors ${
                active
                  ? 'bg-[var(--color-brand)] text-[var(--color-on-primary)]'
                  : 'text-[var(--color-text-secondary)] hover:bg-black/[0.05] dark:hover:bg-white/[0.08]'
              }`}
            >
              <span
                className={`material-symbols-outlined text-[13px] ${
                  highlight && !active ? 'animate-pulse text-red-500' : ''
                }`}
              >
                {TAB_ICONS[key]}
              </span>
              {t(TAB_KEYS[key])}
            </button>
          )
        })}
      </div>

      {tab === 'task' && (
        <ComputerComposer onHeightChange={noopHeight} onSubmitted={onSubmitted} />
      )}
      {tab === 'record' && <RecorderPane />}
      {tab === 'skills' && <SkillsPane onReplayStarted={onSubmitted} />}
    </div>
  )
}
