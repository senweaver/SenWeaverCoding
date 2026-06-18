// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { createPortal } from 'react-dom'
import { useFileDragStore } from '../../stores/fileDragStore'
import { refIconName, refKind } from '../chat/composerRefs'

export function FileDragGhost() {
  const payload = useFileDragStore((s) => s.payload)
  const pointer = useFileDragStore((s) => s.pointer)
  if (!payload || !pointer) return null
  const kind = refKind(payload.relPath)
  const isSession = kind === 'session'
  const bgClass = isSession
    ? 'bg-[var(--color-ref-chip-session-bg)]'
    : 'bg-[var(--color-surface-container-high)]'
  const iconName = payload.isDir && !isSession ? 'folder' : refIconName(payload.relPath)
  return createPortal(
    <div
      className={`pointer-events-none fixed z-[9999] flex max-w-[240px] items-center gap-1.5 rounded-md ${bgClass} px-2 py-1 text-[11px] font-medium text-[var(--color-text-primary)] shadow-[var(--shadow-dropdown)]`}
      style={{ left: pointer.x + 12, top: pointer.y + 12 }}
    >
      <span className="material-symbols-outlined text-[14px] leading-none text-[var(--color-text-secondary)]">
        {iconName}
      </span>
      <span className="truncate">{payload.name}</span>
    </div>,
    document.body,
  )
}
