// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useChatStore } from '../stores/chatStore'
import { useWorkersStore } from '../stores/workersStore'
import { useTabStore } from '../stores/tabStore'
import { useTranslation } from '../i18n'
import { workersApi } from '../api/workers'
import { MessageList } from '../components/chat/MessageList'
import { SectionErrorBoundary } from '../components/layout/SectionErrorBoundary'
import type { WorkerSnapshot } from '../types/chat'

type Props = {
  workerId: string
}

function statusColor(status: WorkerSnapshot['status']): string {
  switch (status) {
    case 'running':
    case 'pending':
      return 'var(--color-warning)'
    case 'completed':
      return 'var(--color-success)'
    case 'failed':
      return 'var(--color-error)'
    case 'stopped':
      return 'var(--color-text-tertiary)'
    default:
      return 'var(--color-text-tertiary)'
  }
}

export function WorkerSession({ workerId }: Props) {
  const t = useTranslation()
  const connectToWorker = useChatStore((s) => s.connectToWorker)
  const disconnectSession = useChatStore((s) => s.disconnectSession)
  const closeTab = useTabStore((s) => s.closeTab)
  const stopWorker = useWorkersStore((s) => s.stopWorker)
  const upsertWorker = useWorkersStore((s) => s.upsertWorker)
  const snapshot = useWorkersStore((s) => s.workersById[workerId])
  const [stopping, setStopping] = useState(false)
  const [hydrated, setHydrated] = useState(false)

  useEffect(() => {
    let cancelled = false
    if (!snapshot) {
      void workersApi
        .get(workerId)
        .then((res) => {
          if (cancelled) return
          upsertWorker(res.snapshot)
          setHydrated(true)
        })
        .catch(() => {
          if (!cancelled) setHydrated(true)
        })
    } else {
      setHydrated(true)
    }
    return () => {
      cancelled = true
    }
  }, [workerId, snapshot, upsertWorker])

  useEffect(() => {
    connectToWorker(workerId)
    return () => {
      disconnectSession(workerId)
    }
  }, [workerId, connectToWorker, disconnectSession])

  const status = snapshot?.status ?? 'pending'
  const title = snapshot?.title ?? workerId
  const model = snapshot?.model ?? ''
  const lastAction = snapshot?.lastAction ?? ''
  const lastDetail = snapshot?.lastDetail ?? ''
  const isTerminal =
    status === 'completed' || status === 'failed' || status === 'stopped'

  const handleStop = async () => {
    if (stopping || isTerminal) return
    setStopping(true)
    try {
      await stopWorker(workerId)
    } finally {
      setStopping(false)
    }
  }

  const handleClose = () => {
    disconnectSession(workerId)
    closeTab(workerId)
  }

  return (
    <div className="flex-1 flex flex-col relative overflow-hidden bg-background text-on-surface">
      <div className="shrink-0 border-b border-[var(--color-border)] bg-[var(--color-surface-container)]">
        <div className="mx-auto max-w-[860px] flex items-center justify-between gap-4 px-8 py-2">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-3">
              <span
                className="h-2 w-2 rounded-full flex-shrink-0"
                style={{
                  backgroundColor: statusColor(status),
                  animation:
                    status === 'running' || status === 'pending'
                      ? 'pulse-dot 1.4s ease-in-out infinite'
                      : undefined,
                }}
              />
              <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
                smart_toy
              </span>
              <span className="text-sm font-semibold text-[var(--color-text-primary)] truncate">
                {title}
              </span>
              {model && (
                <span className="text-[10px] text-[var(--color-text-tertiary)] truncate">
                  {model}
                </span>
              )}
              <span className="text-[10px] text-[var(--color-text-tertiary)] uppercase">
                {status}
              </span>
            </div>
            {(lastAction || lastDetail) && (
              <p className="mt-1 text-[11px] text-[var(--color-text-tertiary)] truncate">
                {lastAction ? `${lastAction} · ` : ''}
                {lastDetail}
              </p>
            )}
          </div>
          <div className="flex items-center gap-2 flex-shrink-0">
            {!isTerminal && (
              <button
                type="button"
                onClick={handleStop}
                disabled={stopping}
                className="inline-flex items-center gap-1 px-2.5 py-1 text-xs rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
              >
                <span className="material-symbols-outlined text-[14px]">stop</span>
                {t('chat.workers.stopWorker') || 'Stop'}
              </button>
            )}
            <button
              type="button"
              onClick={handleClose}
              className="inline-flex items-center gap-1 px-2.5 py-1 text-xs rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
            >
              <span className="material-symbols-outlined text-[14px]">close</span>
              {t('tabs.close') || 'Close'}
            </button>
          </div>
        </div>
      </div>

      {hydrated ? (
        <SectionErrorBoundary label="MessageList" resetKeys={[workerId]}>
          <MessageList sessionId={workerId} />
        </SectionErrorBoundary>
      ) : (
        <div className="flex flex-1 items-center justify-center text-[var(--color-text-tertiary)] text-sm">
          {t('common.loading') || 'Loading...'}
        </div>
      )}
    </div>
  )
}
