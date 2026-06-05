// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useChatStore } from '../../stores/chatStore'
import { useTranslation } from '../../i18n'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import type { AgentTaskNotification } from '../../types/chat'

type Props = {
  sessionId: string
}

async function openLocalPath(path: string): Promise<void> {
  if (!path) return
  if (!isTauriRuntime()) {
    window.open(path, '_blank')
    return
  }
  try {
    const mod = (await import(/* @vite-ignore */ '@tauri-apps/plugin-shell')) as {
      open: (target: string) => Promise<void>
    }
    await mod.open(path)
  } catch (err) {
    console.warn('[AgentTaskNotifications] open output file failed', err)
  }
}

function statusMeta(status: AgentTaskNotification['status']) {
  switch (status) {
    case 'completed':
      return {
        icon: 'task_alt',
        tone: 'border-emerald-500/40 bg-emerald-500/10 text-emerald-900 dark:border-emerald-400/40 dark:bg-emerald-400/10 dark:text-emerald-100',
        dot: 'bg-emerald-500',
        key: 'chat.taskNotification.completed' as const,
      }
    case 'failed':
      return {
        icon: 'error',
        tone: 'border-red-500/40 bg-red-500/10 text-red-900 dark:border-red-400/40 dark:bg-red-400/10 dark:text-red-100',
        dot: 'bg-red-500',
        key: 'chat.taskNotification.failed' as const,
      }
    default:
      return {
        icon: 'stop_circle',
        tone: 'border-amber-500/40 bg-amber-500/10 text-amber-900 dark:border-amber-400/40 dark:bg-amber-400/10 dark:text-amber-100',
        dot: 'bg-amber-500',
        key: 'chat.taskNotification.stopped' as const,
      }
  }
}

export function AgentTaskNotifications({ sessionId }: Props) {
  const t = useTranslation()
  const notifications = useChatStore(
    (s) => s.sessions[sessionId]?.agentTaskNotifications ?? null,
  )
  const dismiss = useChatStore((s) => s.dismissAgentTaskNotification)

  const items = notifications ? Object.values(notifications) : []
  if (items.length === 0) return null

  return (
    <div className="mb-3 flex w-full max-w-[860px] flex-col gap-2">
      {items.map((n) => {
        const meta = statusMeta(n.status)
        return (
          <div
            key={n.toolUseId}
            role="status"
            aria-live="polite"
            className={`flex flex-col gap-1.5 rounded-md border px-3 py-2 text-sm ${meta.tone}`}
          >
            <div className="flex items-center gap-2">
              <span aria-hidden className={`size-2 flex-shrink-0 rounded-full ${meta.dot}`} />
              <span className="material-symbols-outlined text-[16px]">{meta.icon}</span>
              <span className="font-medium">{t(meta.key)}</span>
              <span className="ml-auto flex items-center gap-2">
                {n.outputFile && (
                  <button
                    type="button"
                    onClick={() => void openLocalPath(n.outputFile as string)}
                    className="rounded border border-current/40 px-2 py-0.5 text-xs font-medium hover:bg-current/10"
                  >
                    {t('chat.taskNotification.openResult')}
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => dismiss(sessionId, n.toolUseId)}
                  className="rounded border border-current/40 px-2 py-0.5 text-xs font-medium hover:bg-current/10"
                >
                  {t('chat.taskNotification.dismiss')}
                </button>
              </span>
            </div>
            {n.summary && (
              <div className="whitespace-pre-wrap break-words text-xs opacity-80">
                {n.summary}
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}
