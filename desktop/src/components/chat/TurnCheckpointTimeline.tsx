// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useChatStore } from '../../stores/chatStore'
import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'

type CheckpointNode = {
  batchId: string
  files: string[]
  additions: number
  deletions: number
  reverted: boolean
}

const EMPTY_MESSAGES: never[] = []

export function TurnCheckpointTimeline({ sessionId }: { sessionId: string }) {
  const t = useTranslation()
  const { messages, chatState } = useChatStore(
    useShallow((s) => ({
      messages: s.sessions[sessionId]?.messages ?? EMPTY_MESSAGES,
      chatState: s.sessions[sessionId]?.chatState ?? 'idle',
    })),
  )
  const revertToTurnCheckpoint = useChatStore((s) => s.revertToTurnCheckpoint)
  const [armedBatchId, setArmedBatchId] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [collapsed, setCollapsed] = useState(false)

  const nodes = useMemo<CheckpointNode[]>(() => {
    let turnStart = 0
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      const m = messages[i]!
      if (m.type === 'user_text') {
        turnStart = i
        break
      }
    }
    const byBatch = new Map<string, CheckpointNode>()
    for (let i = turnStart; i < messages.length; i += 1) {
      const m = messages[i]!
      if (m.type !== 'file_edit' || !m.editBatchId) continue
      const existing = byBatch.get(m.editBatchId)
      if (existing) {
        if (!existing.files.includes(m.path)) existing.files.push(m.path)
        existing.additions += m.additions
        existing.deletions += m.deletions
        existing.reverted = existing.reverted && m.reverted === true
      } else {
        byBatch.set(m.editBatchId, {
          batchId: m.editBatchId,
          files: [m.path],
          additions: m.additions,
          deletions: m.deletions,
          reverted: m.reverted === true,
        })
      }
    }
    return Array.from(byBatch.values())
  }, [messages])

  if (chatState !== 'idle' || nodes.length < 2) return null

  const liveNodes = nodes.filter((n) => !n.reverted)
  if (liveNodes.length < 2) return null

  const handleRevertTo = (index: number) => {
    const suffix = nodes.slice(index + 1).filter((n) => !n.reverted)
    if (suffix.length === 0) return
    if (armedBatchId !== nodes[index]!.batchId) {
      setArmedBatchId(nodes[index]!.batchId)
      return
    }
    setArmedBatchId(null)
    setBusy(true)
    revertToTurnCheckpoint(
      sessionId,
      suffix.map((n) => n.batchId),
    )
      .catch((error) => {
        useUIStore.getState().addToast({
          type: 'error',
          message:
            error instanceof Error ? error.message : t('checkpoint.revertFailed'),
        })
      })
      .finally(() => setBusy(false))
  }

  const fileLabel = (node: CheckpointNode) => {
    const first = node.files[0] ?? ''
    const name = first.split(/[\\/]/).pop() ?? first
    return node.files.length > 1
      ? `${name} +${node.files.length - 1}`
      : name
  }

  return (
    <div className="mx-auto w-full max-w-3xl px-4 pb-1">
      <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]/80 px-3 py-1.5">
        <button
          className="flex w-full items-center gap-2 text-[10px] uppercase tracking-wider text-[var(--color-text-tertiary)]"
          onClick={() => setCollapsed((v) => !v)}
        >
          <span className="material-symbols-outlined text-[13px]">timeline</span>
          <span>{t('checkpoint.title')}</span>
          <span className="ml-auto material-symbols-outlined text-[13px]">
            {collapsed ? 'expand_more' : 'expand_less'}
          </span>
        </button>
        {!collapsed && (
          <div className="mt-1 flex items-center gap-1 overflow-x-auto pb-0.5">
            {nodes.map((node, index) => {
              const isLast = index === nodes.length - 1
              const armed = armedBatchId === node.batchId
              const canRevertHere =
                !node.reverted && !isLast && nodes.slice(index + 1).some((n) => !n.reverted)
              return (
                <div key={node.batchId} className="flex shrink-0 items-center gap-1">
                  <div
                    className={`flex items-center gap-1.5 rounded-md border px-2 py-1 ${
                      node.reverted
                        ? 'border-[var(--color-border)] opacity-45'
                        : 'border-[var(--color-outline-variant)]/50 bg-[var(--color-surface-container)]'
                    }`}
                    title={node.files.join('\n')}
                  >
                    <span
                      className={`text-[11px] font-medium ${
                        node.reverted
                          ? 'text-[var(--color-text-tertiary)] line-through'
                          : 'text-[var(--color-text-secondary)]'
                      }`}
                    >
                      {fileLabel(node)}
                    </span>
                    <span className="text-[10px] tabular-nums text-[var(--color-success,#3fb950)]">
                      +{node.additions}
                    </span>
                    <span className="text-[10px] tabular-nums text-[var(--color-error)]">
                      -{node.deletions}
                    </span>
                    {node.reverted && (
                      <span className="text-[9px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
                        {t('checkpoint.revertedTag')}
                      </span>
                    )}
                    {canRevertHere && (
                      <button
                        disabled={busy}
                        onClick={() => handleRevertTo(index)}
                        className={`rounded px-1.5 py-0.5 text-[10px] font-semibold transition-colors ${
                          armed
                            ? 'bg-[var(--color-error)] text-white'
                            : 'bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)] hover:bg-[var(--color-error)]/15 hover:text-[var(--color-error)]'
                        } disabled:opacity-40`}
                        title={t('checkpoint.revertHint')}
                      >
                        {armed ? t('checkpoint.confirmRevert') : t('checkpoint.revertHere')}
                      </button>
                    )}
                  </div>
                  {!isLast && (
                    <span className="material-symbols-outlined text-[12px] text-[var(--color-text-tertiary)]">
                      chevron_right
                    </span>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
