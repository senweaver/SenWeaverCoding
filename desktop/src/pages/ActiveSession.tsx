// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useTabStore } from '../stores/tabStore'
import { useSessionStore } from '../stores/sessionStore'
import { useChatStore } from '../stores/chatStore'
import { useCLITaskStore } from '../stores/cliTaskStore'
import { useTeamStore } from '../stores/teamStore'
import { useTranslation } from '../i18n'
import { resolveSessionTitle } from '../utils/sessionTitle'
import { MessageList } from '../components/chat/MessageList'
import { SectionErrorBoundary } from '../components/layout/SectionErrorBoundary'
import { ChatInput } from '../components/chat/ChatInput'
import { TurnCheckpointTimeline } from '../components/chat/TurnCheckpointTimeline'
import { StreamingEditPreview } from '../components/chat/StreamingEditPreview'
import { DebugResultCard } from '../components/debug/DebugResultCard'
import { TeamStatusBar } from '../components/teams/TeamStatusBar'
import { SessionTaskBar } from '../components/chat/SessionTaskBar'
import { ActivePlanStickyBar } from '../components/chat/ActivePlanStickyBar'
import { ResourceWaitBanner } from '../components/chat/ResourceWaitBanner'
const TASK_POLL_INTERVAL_MS = 10_000
const EMPTY_TOKEN_USAGE = { input_tokens: 0, output_tokens: 0 }

export function ActiveSession() {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessions = useSessionStore((s) => s.sessions)
  const connectToSession = useChatStore((s) => s.connectToSession)
  const { chatState, totalTokens, cumulativeCostUsd, hasMessages, hasStreamingText } =
    useChatStore(
      useShallow((s) => {
        const st = activeTabId ? s.sessions[activeTabId] : undefined
        const usage = st?.tokenUsage ?? EMPTY_TOKEN_USAGE
        return {
          chatState: st?.chatState ?? 'idle',
          totalTokens: usage.input_tokens + usage.output_tokens,
          cumulativeCostUsd: st?.cumulativeCostUsd ?? 0,
          hasMessages: (st?.messages?.length ?? 0) > 0,
          hasStreamingText: !!st?.streamingText,
        }
      }),
    )
  const fetchSessionTasks = useCLITaskStore((s) => s.fetchSessionTasks)
  const hasIncompleteTasks = useCLITaskStore((s) => {
    if (!activeTabId) return false
    const tasks = s.tasksBySessionId[activeTabId]
    if (!tasks || tasks.length === 0) return false
    return tasks.some((task) => task.status !== 'completed')
  })

  const session = sessions.find((s) => s.id === activeTabId)
  const memberInfo = useTeamStore((s) => activeTabId ? s.getMemberBySessionId(activeTabId) : null)
  const activeTeam = useTeamStore((s) => s.activeTeam)
  const isMemberSession = !!memberInfo

  useEffect(() => {
    if (activeTabId && !isMemberSession) {
      connectToSession(activeTabId)
    }
  }, [activeTabId, isMemberSession, connectToSession])

  useEffect(() => {
    if (!activeTabId || isMemberSession) return

    const shouldPollTasks = chatState !== 'idle' || hasIncompleteTasks

    if (!shouldPollTasks) {
      void fetchSessionTasks(activeTabId)
      return
    }

    void fetchSessionTasks(activeTabId)

    const timer = setInterval(() => {
      if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return
      void fetchSessionTasks(activeTabId)
    }, TASK_POLL_INTERVAL_MS)

    return () => clearInterval(timer)
  }, [
    activeTabId,
    isMemberSession,
    chatState,
    hasIncompleteTasks,
    fetchSessionTasks,
  ])

  const t = useTranslation()
  const isEmpty = !hasMessages && !hasStreamingText

  const isActive = chatState !== 'idle'

  const lastUpdated = useMemo(() => {
    if (!session?.modifiedAt) return ''
    const diff = Date.now() - new Date(session.modifiedAt).getTime()
    if (diff < 60000) return t('session.timeJustNow')
    if (diff < 3600000) return t('session.timeMinutes', { n: Math.floor(diff / 60000) })
    if (diff < 86400000) return t('session.timeHours', { n: Math.floor(diff / 3600000) })
    return t('session.timeDays', { n: Math.floor(diff / 86400000) })
  }, [session?.modifiedAt, t])

  if (!activeTabId) return null

  return (
    <div className="flex-1 flex flex-col relative overflow-hidden bg-background text-on-surface">
      {isMemberSession && (
        <div className="shrink-0 border-b border-[var(--color-border)] bg-[var(--color-surface-container)]">
          <div className="mx-auto max-w-[860px] flex items-center justify-between gap-4 px-8 py-2">
            <div className="min-w-0">
              <div className="flex items-center gap-3">
                {memberInfo?.status === 'running' && (
                  <span className="flex h-2 w-2 rounded-full bg-[var(--color-warning)] animate-pulse-dot" />
                )}
                {memberInfo?.status === 'completed' && (
                  <span className="material-symbols-outlined text-[14px] text-[var(--color-success)]" style={{ fontVariationSettings: "'FILL' 1" }}>check_circle</span>
                )}
                <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">smart_toy</span>
                <span className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {memberInfo?.role}
                </span>
                {activeTeam && (
                  <span className="text-[10px] text-[var(--color-text-tertiary)]">
                    @ {activeTeam.name}
                  </span>
                )}
              </div>
              <p className="mt-1 text-[11px] text-[var(--color-text-tertiary)]">
                {t('teams.memberSessionHint')}
              </p>
            </div>
            <button
              onClick={() => {
                if (activeTeam?.leadSessionId) {
                  useTabStore.getState().openTab(
                    activeTeam.leadSessionId,
                    t('teams.leader'),
                    'session',
                  )
                }
              }}
              disabled={!activeTeam?.leadSessionId}
              className="flex shrink-0 items-center gap-1 text-xs font-medium text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] transition-colors disabled:opacity-50 disabled:hover:text-[var(--color-text-secondary)]"
            >
              <span className="material-symbols-outlined text-[14px]">arrow_back</span>
              {t('teams.backToLeader')}
            </button>
          </div>
        </div>
      )}

      {isEmpty ? (
        <div className="flex flex-1 flex-col items-center justify-center p-8 pb-32">
          <div className="flex max-w-md flex-col items-center text-center">
            {isMemberSession ? (
              <>
                <span className="material-symbols-outlined text-[48px] mb-4 text-[var(--color-text-tertiary)]">smart_toy</span>
                <p className="text-[var(--color-text-secondary)]">
                  {memberInfo?.status === 'running'
                    ? `${memberInfo.role} ${t('teams.working')}`
                    : t('teams.noMessages')}
                </p>
              </>
            ) : (
              <>
                <img src="/app-icon.png" alt="SenWeaverCoding" className="mb-6 h-24 w-24" />
                <h1 className="mb-2 text-3xl font-extrabold tracking-tight text-[var(--color-text-primary)]" style={{ fontFamily: 'var(--font-headline)' }}>
                  {t('empty.title')}
                </h1>
                <p className="mx-auto max-w-xs text-[var(--color-text-secondary)]" style={{ fontFamily: 'var(--font-body)' }}>
                  {t('empty.subtitle')}
                </p>
              </>
            )}
          </div>
        </div>
      ) : (
        <>
          {!isMemberSession && (
            <div className="mx-auto flex w-full max-w-[860px] items-center border-b border-outline-variant/10 px-8 py-3">
              <div className="flex-1">
                <h1 className="text-lg font-bold font-headline text-on-surface leading-tight">
                  {resolveSessionTitle(session?.title, t('session.untitled'))}
                </h1>
                <div className="flex items-center gap-2 text-[10px] text-outline font-medium mt-1">
                  {isActive && (
                    <span className="flex items-center gap-1">
                      <span className="w-1.5 h-1.5 rounded-full bg-[var(--color-success)] animate-pulse-dot" />
                      {t('session.active')}
                    </span>
                  )}
                  {totalTokens > 0 && (
                    <>
                      <span className="text-[var(--color-outline)]">·</span>
                      <span title={t('session.lastTurnTokens')}>
                        {totalTokens.toLocaleString()} t
                      </span>
                    </>
                  )}
                  {cumulativeCostUsd > 0 && (
                    <>
                      <span className="text-[var(--color-outline)]">·</span>
                      <span title={t('session.sessionCost')}>
                        ${cumulativeCostUsd < 0.01
                          ? cumulativeCostUsd.toFixed(4)
                          : cumulativeCostUsd.toFixed(2)}
                      </span>
                    </>
                  )}
                  {lastUpdated && (
                    <>
                      <span className="text-[var(--color-outline)]">·</span>
                      <span>{t('session.lastUpdated', { time: lastUpdated })}</span>
                    </>
                  )}
                  {session?.messageCount !== undefined && session.messageCount > 0 && (
                    <>
                      <span className="text-[var(--color-outline)]">·</span>
                      <span>{t('session.messages', { count: session.messageCount })}</span>
                    </>
                  )}
                </div>
                {session?.workDirExists === false && (
                  <div className="mt-2 inline-flex max-w-full items-center gap-2 rounded-lg border border-[var(--color-error)]/20 bg-[var(--color-error)]/8 px-3 py-1.5 text-[11px] text-[var(--color-error)]">
                    <span className="material-symbols-outlined text-[14px]">warning</span>
                    <span className="truncate">
                      {t('session.workspaceUnavailable', { dir: session.workDir || 'directory no longer exists' })}
                    </span>
                  </div>
                )}
              </div>
            </div>
          )}

          {isMemberSession ? (
            <div className="relative flex flex-1 min-h-0 flex-col">
              <SectionErrorBoundary label="MessageList" resetKeys={[activeTabId]}>
                <MessageList />
              </SectionErrorBoundary>
            </div>
          ) : (
            <div className="relative flex flex-1 min-h-0 flex-col">
              <SectionErrorBoundary label="MessageList" resetKeys={[activeTabId]}>
                <MessageList sessionId={activeTabId} />
              </SectionErrorBoundary>
            </div>
          )}
        </>
      )}

      {!isMemberSession && <SessionTaskBar />}

      {!isMemberSession && (
        <SectionErrorBoundary label="ActivePlanStickyBar" resetKeys={[activeTabId]}>
          <ActivePlanStickyBar sessionId={activeTabId} />
        </SectionErrorBoundary>
      )}

      <TeamStatusBar />

      {!isMemberSession && activeTabId && (
        <SectionErrorBoundary label="ResourceWaitBanner" resetKeys={[activeTabId]}>
          <ResourceWaitBanner sessionId={activeTabId} />
        </SectionErrorBoundary>
      )}

      {!isMemberSession && activeTabId && (
        <SectionErrorBoundary label="DebugResultCard" resetKeys={[activeTabId]}>
          <DebugResultCard sessionId={activeTabId} />
        </SectionErrorBoundary>
      )}

      {activeTabId && !isMemberSession && (
        <SectionErrorBoundary label="StreamingEditPreview" resetKeys={[activeTabId]}>
          <StreamingEditPreview sessionId={activeTabId} />
        </SectionErrorBoundary>
      )}
      {activeTabId && !isMemberSession && (
        <SectionErrorBoundary label="TurnCheckpointTimeline" resetKeys={[activeTabId]}>
          <TurnCheckpointTimeline sessionId={activeTabId} />
        </SectionErrorBoundary>
      )}
      <SectionErrorBoundary label="ChatInput" resetKeys={[activeTabId]}>
        <ChatInput variant={isEmpty && !isMemberSession ? 'hero' : 'default'} />
      </SectionErrorBoundary>
    </div>
  )
}
