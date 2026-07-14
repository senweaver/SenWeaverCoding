// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useRef, useEffect, useLayoutEffect, useMemo, memo, useState, useCallback } from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'
import { useShallow } from 'zustand/react/shallow'
import { ApiError } from '../../api/client'
import { sessionsApi, type SessionRewindResponse } from '../../api/sessions'
import {
  useChatStore,
  isAskQuestionToolName,
  MAX_IN_MEMORY_MESSAGES,
} from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useTeamStore } from '../../stores/teamStore'
import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'
import type { TranslationKey } from '../../i18n/locales/en'
import { UserMessage } from './UserMessage'
import { InlineUserMessageEditor } from './InlineUserMessageEditor'
import { AssistantMessage } from './AssistantMessage'
import { ThinkingBlock, ActiveThinkingBlock } from './ThinkingBlock'
import { ToolCard } from './tools/ToolCard'
import { ExploredCard, buildExploredSummary, type ExploredSummary } from './ExploredCard'
import { ToolResultBlock } from './ToolResultBlock'
import { PermissionDialog } from './PermissionDialog'
import { FileEditNotification } from './FileEditNotification'
import { CommandPreviewCard } from './CommandPreviewCard'
import { SubagentChunkBlock } from './SubagentChunkBlock'
import { AskUserQuestion } from './AskUserQuestion'
import { StreamingIndicator } from './StreamingIndicator'
import { ProviderRetryBanner } from './ProviderRetryBanner'
import { AgentTaskNotifications } from './AgentTaskNotifications'
import { InlineTaskSummary } from './InlineTaskSummary'
import { AnswersCard } from './AnswersCard'
import { PlanCard } from './PlanCard'
import { PlanProgressCard } from './PlanProgressCard'
import { CuratorCard } from './CuratorCard'
import { parseCuratorEnvelope } from '../../utils/parseCuratorMd'
import { ModeSwitchCard } from './ModeSwitchCard'
import { PairCheckpointCard } from './PairCheckpointCard'
import { PlanModeBlockedNotice } from './PlanModeBlockedNotice'
import { SectionErrorBoundary } from '../layout/SectionErrorBoundary'
import type { AttachmentRef, UIMessage } from '../../types/chat'
import { Modal } from '../shared/Modal'
import { Button } from '../shared/Button'
import { isExploreToolName } from '../../utils/toolCategory'
import {
  buildAssistantTurnCopyMap,
  type AssistantTurnCopyInfo,
} from '../../utils/assistantTurnCopy'

type ToolCall = Extract<UIMessage, { type: 'tool_use' }>
type ToolResult = Extract<UIMessage, { type: 'tool_result' }>

const RETRY_BANNER_MIN_ATTEMPT = 3

type RenderItem =
  | { kind: 'explored'; id: string; items: UIMessage[]; summary: ExploredSummary }
  | { kind: 'message'; message: UIMessage }

const TODO_LIST_TOOL_NAMES = new Set([
  'todo_write',
  'TodoWrite',
  'todowrite',
  'tasks_write',
  'TasksWrite',
])

const USER_FACING_ERROR_CODES = new Set([
  'NO_MODEL_CONFIGURED',
  'CONFIG_ERROR',
  'CONNECTION_FAILED',
  'CONNECTION_TIMEOUT',
  'INSUFFICIENT_BALANCE',
  'GATEWAY_ERROR',
  'AUTH_ERROR',
  'RATE_LIMITED',
  'MODEL_UNAVAILABLE',
  'ENGINE_OVERLOADED',
  'PROVIDER_FAILOVER',
  'AGENT_TURN_FAILED',
  'UNKNOWN_ERROR',
  'TURN_CANCELLED',
  'VALIDATION_ERROR',
])

function resolveErrorDisplay(
  message: { message: string; code: string; detail?: string },
  t: (key: TranslationKey) => string,
): { friendly: string; technicalDetail: string | null } {
  const errorKey = message.code ? (`error.${message.code}` as TranslationKey) : null
  const i18nText = errorKey ? t(errorKey) : null
  const hasI18n = Boolean(
    errorKey &&
      i18nText &&
      i18nText !== errorKey &&
      USER_FACING_ERROR_CODES.has(message.code),
  )
  const friendly = hasI18n ? i18nText! : message.message.trim() || t('error.UNKNOWN_ERROR')
  const technicalDetail =
    message.detail?.trim() ||
    (message.message.trim() && message.message.trim() !== friendly ? message.message.trim() : null)
  return { friendly, technicalDetail }
}

function signatureMatches(
  a: Map<string, Extract<UIMessage, { type: 'tool_result' }>>,
  b: Map<string, Extract<UIMessage, { type: 'tool_result' }>>,
): boolean {
  if (a.size !== b.size) return false
  for (const [k, va] of a.entries()) {
    const vb = b.get(k)
    if (vb !== va) return false
  }
  return true
}

type RenderModel = {
  renderItems: RenderItem[]
  toolResultMap: Map<string, ToolResult>
  childToolCallsByParent: Map<string, ToolCall[]>
}

function appendChildToolCall(
  childToolCallsByParent: Map<string, ToolCall[]>,
  parentToolUseId: string,
  toolCall: ToolCall,
) {
  const siblings = childToolCallsByParent.get(parentToolUseId)
  if (siblings) siblings.push(toolCall)
  else childToolCallsByParent.set(parentToolUseId, [toolCall])
}

export function buildRenderModel(messages: UIMessage[]): RenderModel {
  const items: RenderItem[] = []
  const toolResultMap = new Map<string, ToolResult>()
  const childToolCallsByParent = new Map<string, ToolCall[]>()
  const seenToolUseIds = new Set<string>()
  const emittedToolUseIds = new Set<string>()
  let buffer: UIMessage[] = []
  let bufferToolUseIds = new Set<string>()

  for (const msg of messages) {
    if (msg.type === 'tool_use') seenToolUseIds.add(msg.toolUseId)
    if (msg.type === 'tool_result') toolResultMap.set(msg.toolUseId, msg)
  }

  const primaryCount = (entries: UIMessage[]) =>
    entries.reduce((acc, m) => acc + (m.type === 'tool_result' ? 0 : 1), 0)

  const flush = () => {
    if (buffer.length === 0) return
    const primary = primaryCount(buffer)
    if (primary <= 1) {

      for (const only of buffer) {
        if (only.type === 'tool_result') continue
        items.push({ kind: 'message', message: only })
        if (only.type === 'tool_use') emittedToolUseIds.add(only.toolUseId)
      }
    } else {
      const summary = buildExploredSummary(buffer)
      // Key the group solely by its first message id (stable and unique), NOT by
      // ordinal position: a positional counter would change every retained
      // group's key when older messages are trimmed from the front, forcing
      // Virtuoso to remount the whole list.
      items.push({
        kind: 'explored',
        id: `explored-${buffer[0]?.id ?? 'empty'}`,
        items: buffer.filter((m) => m.type !== 'tool_result'),
        summary,
      })
      for (const item of buffer) {
        if (item.type === 'tool_use') emittedToolUseIds.add(item.toolUseId)
      }
    }
    buffer = []
    bufferToolUseIds = new Set<string>()
  }

  for (const msg of messages) {
    if (msg.type === 'tool_result') {
      if (bufferToolUseIds.has(msg.toolUseId)) {

        continue
      }
      if (seenToolUseIds.has(msg.toolUseId)) continue

      flush()
      items.push({ kind: 'message', message: msg })
      continue
    }

    if (msg.type === 'tool_use') {

      if (msg.parentToolUseId && (emittedToolUseIds.has(msg.parentToolUseId) || bufferToolUseIds.has(msg.parentToolUseId))) {
        appendChildToolCall(childToolCallsByParent, msg.parentToolUseId, msg)
        emittedToolUseIds.add(msg.toolUseId)
        continue
      }
      if (isAskQuestionToolName(msg.toolName)) {

        flush()
        if (toolResultMap.has(msg.toolUseId)) {
          items.push({ kind: 'message', message: msg })
        }
        emittedToolUseIds.add(msg.toolUseId)
        continue
      }
      if (isExploreToolName(msg.toolName)) {
        buffer.push(msg)
        bufferToolUseIds.add(msg.toolUseId)
        continue
      }
      flush()
      items.push({ kind: 'message', message: msg })
      emittedToolUseIds.add(msg.toolUseId)
      continue
    }

    if (msg.type === 'thinking') {
      buffer.push(msg)
      continue
    }

    flush()
    items.push({ kind: 'message', message: msg })
  }

  flush()

  let modeSwitchIdx = -1
  let modeSwitchPlanPath: string | null = null
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i]!
    if (it.kind !== 'message') continue
    if (it.message.type === 'mode_switch_card') {
      modeSwitchIdx = i
      modeSwitchPlanPath = it.message.planPath || null
      break
    }
  }

  if (modeSwitchIdx >= 0 && modeSwitchPlanPath) {

    let planIdx = -1
    for (let i = modeSwitchIdx - 1; i >= 0; i--) {
      const it = items[i]!
      if (it.kind !== 'message') continue
      const m = it.message
      if (
        m.type === 'plan_card' &&
        m.planPath === modeSwitchPlanPath &&
        (m.status === 'writing' || m.status === 'completed')
      ) {
        planIdx = i
        break
      }
      if (
        m.type === 'curator_card' &&
        m.implBlueprintPath === modeSwitchPlanPath
      ) {
        planIdx = i
        break
      }
    }
    if (planIdx >= 0 && planIdx < modeSwitchIdx - 1) {
      const planItem = items[planIdx]!
      items.splice(planIdx, 1)

      items.splice(modeSwitchIdx - 1, 0, planItem)
    }
  } else {

    let activePlanIdx = -1
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i]!
      if (it.kind !== 'message') continue
      const m = it.message
      if (
        m.type === 'plan_card' &&
        (m.status === 'writing' || m.status === 'completed')
      ) {
        activePlanIdx = i
        break
      }
      if (m.type === 'curator_card') {
        activePlanIdx = i
        break
      }
    }
    if (activePlanIdx >= 0) {
      const activeItem = items[activePlanIdx]!
      const activeMsg = activeItem.kind === 'message' ? activeItem.message : null
      const isCurator = activeMsg !== null && activeMsg.type === 'curator_card'
      const keepInPlace =
        activeMsg !== null &&
        activeMsg.type === 'curator_card' &&
        activeMsg.status === 'writing'
      if (!keepInPlace) {
        let nextUserIdx = items.length
        for (let i = activePlanIdx + 1; i < items.length; i++) {
          const it = items[i]!
          if (it.kind === 'message' && it.message.type === 'user_text') {
            nextUserIdx = i
            break
          }
        }
        let anchor: RenderItem | null =
          nextUserIdx < items.length ? items[nextUserIdx]! : null
        if (isCurator) {
          let turnStart = 0
          for (let i = activePlanIdx - 1; i >= 0; i--) {
            const it = items[i]!
            if (it.kind === 'message' && it.message.type === 'user_text') {
              turnStart = i + 1
              break
            }
          }
          let tailAssistantIdx = -1
          for (let i = turnStart; i < nextUserIdx; i++) {
            if (i === activePlanIdx) continue
            const it = items[i]!
            if (it.kind === 'message' && it.message.type === 'assistant_text') {
              tailAssistantIdx = i
            }
          }
          if (tailAssistantIdx >= 0) anchor = items[tailAssistantIdx]!
        }
        const anchorIdx = anchor ? items.indexOf(anchor) : items.length
        if (activePlanIdx !== anchorIdx - 1) {
          items.splice(activePlanIdx, 1)
          const insertAt = anchor ? items.indexOf(anchor) : items.length
          items.splice(insertAt, 0, activeItem)
        }
      }
    }
  }

  const todoItemIdxs: number[] = []
  for (let i = 0; i < items.length; i++) {
    const it = items[i]!
    if (
      it.kind === 'message' &&
      it.message.type === 'tool_use' &&
      TODO_LIST_TOOL_NAMES.has(it.message.toolName)
    ) {
      todoItemIdxs.push(i)
    }
  }
  if (todoItemIdxs.length > 1) {
    for (let k = 0; k < todoItemIdxs.length - 1; k++) {
      const idx = todoItemIdxs[k]!
      const it = items[idx]!
      if (it.kind === 'message' && !it.message.superseded) {
        items[idx] = { ...it, message: { ...it.message, superseded: true } }
      }
    }
  }

  return { renderItems: items, toolResultMap, childToolCallsByParent }
}

type MessageListProps = {
  sessionId?: string | null
}

const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 160
const USER_SCROLL_UP_CANCEL_PX = 24

// At idle, suppress follow-on-growth briefly after the user clicks inside the
// list (e.g. expanding a tool card): the resulting height change is their own
// interaction, and snapping to the bottom would yank the content they are
// trying to read. Streaming growth is unaffected.
const IDLE_INTERACTION_GRACE_MS = 1200
// Follow-mode re-arms once the user manually returns close to the bottom.
// Cancellation is gesture-driven (wheel/drag/keys), so a slightly generous
// band here cannot fight the user; 48px is reachable by hand while staying
// well inside atBottomThreshold (160px).
const AUTO_SCROLL_REARM_THRESHOLD_PX = 48
const FIRST_ITEM_INDEX_BASE = 1_000_000

const EMPTY_MESSAGES: UIMessage[] = []
const EMPTY_SUBAGENT_TIMELINES: Record<string, never> = {}

function renderItemKey(item: RenderItem): string {
  return item.kind === 'explored' ? item.id : item.message.id
}

type ListFooterContext = {
  streamingText: string
  isStreaming: boolean
  resolvedSessionId: string | null
  showRetryBanner: boolean
  showPlanningIndicator: boolean
  awaitingWorkers: boolean
  planningLabel: string
  activeThinkingId: string | null
  onLiveThinkingGrow: () => void
}

function ListHeader() {
  return <div className="h-4" />
}

function ListFooter({ context }: { context?: ListFooterContext }) {
  if (!context) return <div className="h-4" />
  const {
    streamingText,
    isStreaming,
    resolvedSessionId,
    showRetryBanner,
    showPlanningIndicator,
    awaitingWorkers,
    planningLabel,
    activeThinkingId,
    onLiveThinkingGrow,
  } = context
  return (
    <div className="mx-auto w-full max-w-[860px] flow-root px-4 pb-4">
      {resolvedSessionId && activeThinkingId && (
        <ActiveThinkingBlock
          sessionId={resolvedSessionId}
          onContentGrow={onLiveThinkingGrow}
        />
      )}

      {streamingText && (
        <SectionErrorBoundary label="streaming" resetKeys={[resolvedSessionId ?? '']}>
          <AssistantMessage content={streamingText} isStreaming={isStreaming} />
        </SectionErrorBoundary>
      )}

      {resolvedSessionId && showRetryBanner && (
        <ProviderRetryBanner sessionId={resolvedSessionId} />
      )}

      {resolvedSessionId && <AgentTaskNotifications sessionId={resolvedSessionId} />}

      {        showPlanningIndicator && !showRetryBanner && (
        awaitingWorkers ? (
          <div className="mx-auto w-full max-w-[860px] px-8 py-2">
            <div className="inline-flex items-center gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-1.5 text-[12px] text-[var(--color-text-secondary)]">
              <span className="material-symbols-outlined text-[14px] text-[var(--color-warning)] animate-pulse">
                smart_toy
              </span>
              <span>{planningLabel}</span>
            </div>
          </div>
        ) : (
          <StreamingIndicator />
        )
      )}
    </div>
  )
}

const VIRTUOSO_COMPONENTS = { Header: ListHeader, Footer: ListFooter }

export function MessageList({ sessionId }: MessageListProps = {}) {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const resolvedSessionId = sessionId ?? activeTabId
  const {
    messages,
    chatState,
    streamingText,
    activeThinkingId,
    pendingPermission,
    pendingRewind,
    providerRetry,
    pendingSendAfterRewind,
    historyHasMore,
    historyLoadingOlder,
  } = useChatStore(
    useShallow((s) => {
      const st = resolvedSessionId ? s.sessions[resolvedSessionId] : undefined
      return {
        messages: st?.messages ?? EMPTY_MESSAGES,
        chatState: st?.chatState ?? ('idle' as const),
        streamingText: st?.streamingText ?? '',
        activeThinkingId: st?.activeThinkingId ?? null,
        pendingPermission: st?.pendingPermission ?? null,
        pendingRewind: st?.pendingRewind ?? null,
        providerRetry: st?.providerRetry ?? null,
        pendingSendAfterRewind: st?.pendingSendAfterRewind ?? null,
        historyHasMore: st?.historyHasMore === true,
        historyLoadingOlder: st?.historyLoadingOlder === true,
      }
    }),
  )
  const showRetryBanner =
    !!providerRetry && providerRetry.attempt >= RETRY_BANNER_MIN_ATTEMPT
  const stopGeneration = useChatStore((s) => s.stopGeneration)
  const reloadHistory = useChatStore((s) => s.reloadHistory)
  const queueComposerPrefill = useChatStore((s) => s.queueComposerPrefill)
  const sendMessage = useChatStore((s) => s.sendMessage)
  const restoreRewindAction = useChatStore((s) => s.restoreRewind)
  const confirmSendAfterRewind = useChatStore((s) => s.confirmSendAfterRewind)
  const cancelSendAfterRewind = useChatStore((s) => s.cancelSendAfterRewind)
  const isMemberSession = useTeamStore((s) =>
    resolvedSessionId ? Boolean(s.getMemberBySessionId(resolvedSessionId)) : false,
  )
  const activeSessionMeta = useSessionStore((s) =>
    resolvedSessionId ? s.sessions.find((x) => x.id === resolvedSessionId) : undefined,
  )
  const addToast = useUIStore((s) => s.addToast)
  const virtuosoRef = useRef<VirtuosoHandle>(null)
  const scrollerElRef = useRef<HTMLElement | null>(null)
  const scrollerCleanupRef = useRef<(() => void) | null>(null)
  const followRef = useRef(true)
  const atBottomRef = useRef(true)
  const atTopRef = useRef(false)
  const lastScrollTopRef = useRef(0)
  const programmaticScrollRef = useRef(false)
  // True while the user is physically holding a pointer/touch on the scroller
  // (scrollbar drag, touch pan). Only then may upward scroll deltas cancel
  // follow; every other scrollTop shift (Virtuoso re-measurement, streaming
  // footer commits, programmatic pins) must never break stick-to-bottom.
  const userInteractingRef = useRef(false)
  const scrollRafRef = useRef<number | null>(null)
  const followRafRef = useRef<number | null>(null)
  const followForceRef = useRef(false)
  const prevRenderKeysRef = useRef<string[]>([])
  const initialPinPendingRef = useRef(true)
  const initialPinDeadlineRef = useRef(0)
  const lastScrollerPointerDownAtRef = useRef(0)
  const t = useTranslation()
  const [rewindTarget, setRewindTarget] = useState<{
    userMessageIndex: number

    content: string
    attachments?: Extract<UIMessage, { type: 'user_text' }>['attachments']

    pendingNewContent?: string
    pendingNewAttachments?: AttachmentRef[]
  } | null>(null)
  const [rewindPreview, setRewindPreview] = useState<SessionRewindResponse | null>(null)
  const [rewindError, setRewindError] = useState<string | null>(null)
  const [isLoadingPreview, setIsLoadingPreview] = useState(false)
  const [isExecutingRewind, setIsExecutingRewind] = useState(false)

  const [executingRewindChoice, setExecutingRewindChoice] = useState<
    'revert' | 'no-revert' | null
  >(null)
  const [restoreConfirmOpen, setRestoreConfirmOpen] = useState(false)
  const [isRestoring, setIsRestoring] = useState(false)
  const [isCommittingSendAfterRewind, setIsCommittingSendAfterRewind] = useState(false)

  const [editingMessage, setEditingMessage] = useState<{
    messageId: string
    userMessageIndex: number
    content: string
    attachments?: Extract<UIMessage, { type: 'user_text' }>['attachments']
  } | null>(null)

  const [showScrollToBottom, setShowScrollToBottom] = useState(false)

  const [firstItemIndex, setFirstItemIndex] = useState(FIRST_ITEM_INDEX_BASE)

  const scrollFollowToBottom = useCallback((force = false) => {
    if (!force && !followRef.current) return
    if (typeof document !== 'undefined' && document.hidden) return
    if (force) followForceRef.current = true
    if (followRafRef.current != null) return
    followRafRef.current = requestAnimationFrame(() => {
      followRafRef.current = null
      const useForce = followForceRef.current
      followForceRef.current = false
      if (typeof document !== 'undefined' && document.hidden) return
      const node = scrollerElRef.current
      if (!node) return
      if (!useForce && !followRef.current) return
      const target = node.scrollHeight - node.clientHeight
      if (target - node.scrollTop <= 1) return
      programmaticScrollRef.current = true
      node.scrollTop = target
      if (scrollRafRef.current != null) cancelAnimationFrame(scrollRafRef.current)
      scrollRafRef.current = requestAnimationFrame(() => {
        scrollRafRef.current = null
        programmaticScrollRef.current = false
      })
    })
  }, [])

  const handleScrollerRef = useCallback((el: HTMLElement | Window | null) => {
    if (scrollerCleanupRef.current) {
      scrollerCleanupRef.current()
      scrollerCleanupRef.current = null
    }
    scrollerElRef.current = el instanceof HTMLElement ? el : null
    const next = scrollerElRef.current
    if (!next) return

    lastScrollTopRef.current = next.scrollTop
    userInteractingRef.current = false

    const cancelFollow = () => {
      if (!followRef.current) return
      followRef.current = false
      setShowScrollToBottom(true)
    }

    // The scroll listener never cancels follow on its own: Virtuoso item
    // re-measurement, firstItemIndex compensation and streaming-footer commits
    // all shift scrollTop without any user intent, and treating those as
    // "the user scrolled up" was what kept killing stick-to-bottom. An upward
    // delta only cancels while a pointer/touch gesture is physically active
    // (scrollbar drag / touch pan); wheel and keyboard cancel via their own
    // dedicated listeners below.
    const onScroll = () => {
      const st = next.scrollTop
      const distanceFromBottom = next.scrollHeight - st - next.clientHeight
      const movedUp = st < lastScrollTopRef.current
      const draggedUp =
        userInteractingRef.current &&
        !programmaticScrollRef.current &&
        st < lastScrollTopRef.current - USER_SCROLL_UP_CANCEL_PX
      if (draggedUp && followRef.current) {
        cancelFollow()
      } else if (!movedUp && distanceFromBottom <= AUTO_SCROLL_REARM_THRESHOLD_PX) {
        // Re-arm only on non-upward motion into the bottom band, so a fresh
        // wheel-up cancel right at the bottom is not immediately overridden
        // by its own scroll event still being within the band.
        followRef.current = true
        setShowScrollToBottom((prev) => (prev ? false : prev))
      } else if (distanceFromBottom <= AUTO_SCROLL_BOTTOM_THRESHOLD_PX && followRef.current) {
        setShowScrollToBottom((prev) => (prev ? false : prev))
      }
      lastScrollTopRef.current = st
    }

    const onWheel = (e: WheelEvent) => {
      // Ignore wheel-up when there is nothing to scroll back through; a pill
      // with no scrollback would be a dead control.
      if (e.deltaY < 0 && next.scrollHeight - next.clientHeight > 1) {
        cancelFollow()
      }
    }
    const onPointerDown = () => {
      userInteractingRef.current = true
      lastScrollerPointerDownAtRef.current = Date.now()
    }
    const endPointerInteraction = () => {
      userInteractingRef.current = false
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'PageUp' || e.key === 'Home' || e.key === 'ArrowUp') {
        cancelFollow()
      }
    }

    next.addEventListener('scroll', onScroll, { passive: true })
    next.addEventListener('wheel', onWheel, { passive: true })
    next.addEventListener('pointerdown', onPointerDown, { passive: true })
    next.addEventListener('touchstart', onPointerDown, { passive: true })
    next.addEventListener('keydown', onKeyDown)
    // Pointer/touch releases can land outside the scroller (drag ends off the
    // scrollbar), so the end-of-gesture listeners live on the window.
    window.addEventListener('pointerup', endPointerInteraction, { passive: true })
    window.addEventListener('pointercancel', endPointerInteraction, { passive: true })
    window.addEventListener('touchend', endPointerInteraction, { passive: true })
    window.addEventListener('touchcancel', endPointerInteraction, { passive: true })

    scrollerCleanupRef.current = () => {
      next.removeEventListener('scroll', onScroll)
      next.removeEventListener('wheel', onWheel)
      next.removeEventListener('pointerdown', onPointerDown)
      next.removeEventListener('touchstart', onPointerDown)
      next.removeEventListener('keydown', onKeyDown)
      window.removeEventListener('pointerup', endPointerInteraction)
      window.removeEventListener('pointercancel', endPointerInteraction)
      window.removeEventListener('touchend', endPointerInteraction)
      window.removeEventListener('touchcancel', endPointerInteraction)
    }
  }, [])

  // Virtuoso calls scrollerRef(null) on unmount, but keep an unmount sweep as
  // insurance so the window-level gesture listeners can never outlive the list.
  useEffect(() => {
    return () => {
      if (scrollerCleanupRef.current) {
        scrollerCleanupRef.current()
        scrollerCleanupRef.current = null
      }
    }
  }, [])

  const scrollToBottomNow = useCallback(() => {
    followRef.current = true
    atBottomRef.current = true
    setShowScrollToBottom(false)
    // Clear any in-list interaction grace: an explicit "scroll to latest"
    // click overrides it, and follow now persists until a real upward gesture.
    lastScrollerPointerDownAtRef.current = 0
    scrollFollowToBottom(true)
  }, [scrollFollowToBottom])

  useEffect(() => {
    followRef.current = true
    atBottomRef.current = true
    atTopRef.current = false
    userInteractingRef.current = false
    lastScrollerPointerDownAtRef.current = 0
    prevRenderKeysRef.current = []
    initialPinPendingRef.current = true
    initialPinDeadlineRef.current = 0
    setFirstItemIndex(FIRST_ITEM_INDEX_BASE)
    setShowScrollToBottom(false)
  }, [resolvedSessionId])

  const handleLiveThinkingGrow = useCallback(() => {
    if (!followRef.current) return
    if (typeof document !== 'undefined' && document.hidden) return
    scrollFollowToBottom()
  }, [scrollFollowToBottom])

  const maybeLoadOlder = useCallback(() => {
    if (!resolvedSessionId) return
    const st = useChatStore.getState().sessions[resolvedSessionId]
    if (!st || st.historyHasMore !== true || st.historyLoadingOlder === true) return
    void useChatStore.getState().loadOlderHistory(resolvedSessionId)
  }, [resolvedSessionId])

  useEffect(() => {
    if (historyLoadingOlder) return
    if (!historyHasMore) return
    if (!atTopRef.current) return
    maybeLoadOlder()
  }, [historyLoadingOlder, historyHasMore, maybeLoadOlder])

  // Window the in-memory message list once a turn settles: only when the user is
  // pinned to the tail (not reading older messages) and not mid-pagination, so a
  // trim never yanks the viewport. The symmetric firstItemIndex handling above
  // keeps retained rows anchored; loadOlderHistory refetches trimmed messages.
  useEffect(() => {
    if (!resolvedSessionId) return
    if (chatState !== 'idle') return
    if (messages.length <= MAX_IN_MEMORY_MESSAGES) return
    if (!atBottomRef.current || !followRef.current) return
    if (historyLoadingOlder) return
    useChatStore.getState().capMessageWindow(resolvedSessionId)
  }, [resolvedSessionId, chatState, messages.length, historyLoadingOlder])

  useEffect(() => {
    setRewindTarget(null)
    setRewindPreview(null)
    setRewindError(null)
    setIsLoadingPreview(false)
    setIsExecutingRewind(false)
    setExecutingRewindChoice(null)
    setRestoreConfirmOpen(false)
    setIsRestoring(false)
    setIsCommittingSendAfterRewind(false)
    setEditingMessage(null)
  }, [resolvedSessionId])

  useEffect(() => {
    if (typeof document === 'undefined') return
    const onVisibility = () => {
      if (document.hidden) return
      if (!followRef.current) return
      requestAnimationFrame(() => {
        if (document.hidden) return
        if (!followRef.current) return
        scrollFollowToBottom()
      })
    }
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [scrollFollowToBottom])

  useEffect(() => {
    if (!resolvedSessionId || !rewindTarget) return

    let cancelled = false
    setIsLoadingPreview(true)
    setRewindPreview(null)
    setRewindError(null)

    void sessionsApi
      .rewind(resolvedSessionId, {
        userMessageIndex: rewindTarget.userMessageIndex,
        dryRun: true,
      })
      .then((preview) => {
        if (!cancelled) {
          setRewindPreview(preview)
        }
      })
      .catch((error) => {
        if (cancelled) return
        const message =
          error instanceof ApiError
            ? typeof error.body === 'object' && error.body && 'message' in error.body
              ? String((error.body as { message: unknown }).message)
              : error.message
            : error instanceof Error
              ? error.message
              : String(error)
        setRewindError(message)
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoadingPreview(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [resolvedSessionId, rewindTarget])

  const childResultsByParentRef = useRef<Map<string, Map<string, Extract<UIMessage, { type: 'tool_result' }>>>>(new Map())

  const baseMessages = useMemo(() => {
    if (!activeThinkingId) return messages
    let hasActive = false
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (!m) continue
      if (m.type === 'thinking' && m.id === activeThinkingId) {
        hasActive = true
        break
      }
    }
    if (!hasActive) return messages
    return messages.filter(
      (m) => !(m.type === 'thinking' && m.id === activeThinkingId),
    )
  }, [messages, activeThinkingId])

  const { toolResultMap, renderItems, childToolCallsByParent } = useMemo(
    () => buildRenderModel(baseMessages),
    [baseMessages],
  )

  const childResultsByParent = useMemo(() => {
    const cache = childResultsByParentRef.current
    const next = new Map<
      string,
      Map<string, Extract<UIMessage, { type: 'tool_result' }>>
    >()
    for (const [parentId, calls] of childToolCallsByParent.entries()) {
      const built = new Map<string, Extract<UIMessage, { type: 'tool_result' }>>()
      let signature = `${calls.length}`
      for (const c of calls) {
        const r = toolResultMap.get(c.toolUseId)
        if (r) {
          built.set(c.toolUseId, r)
          signature += `|${c.toolUseId}=${r.id}`
        } else {
          signature += `|${c.toolUseId}=∅`
        }
      }
      const previous = cache.get(parentId)
      if (
        previous &&
        previous.size === built.size &&
        signatureMatches(previous, built)
      ) {
        next.set(parentId, previous)
      } else {
        next.set(parentId, built)
        void signature
      }
    }
    childResultsByParentRef.current = next
    return next
  }, [childToolCallsByParent, toolResultMap])

  const listRenderItems = useMemo(() => {
    let end = renderItems.length
    if (chatState === 'idle' && streamingText.trim().length === 0) {
      while (end > 0) {
        const item = renderItems[end - 1]!
        if (item.kind === 'message' && item.message.type === 'thinking') {
          end -= 1
        } else {
          break
        }
      }
    }
    return end === renderItems.length ? renderItems : renderItems.slice(0, end)
  }, [renderItems, chatState, streamingText])

  useLayoutEffect(() => {
    // Keep Virtuoso's firstItemIndex consistent with front-of-list mutations so
    // retained rows keep a stable virtual index (no remount / scroll jump):
    //  - prepend K rows (loadOlderHistory)  -> firstItemIndex -= K
    //  - trim   K rows from the front (cap)  -> firstItemIndex += K
    // Render-item keys are stable (message id / first-message id for groups), so
    // we detect which case happened by locating the anchor key across renders.
    const keys = listRenderItems.map(renderItemKey)
    const prevKeys = prevRenderKeysRef.current
    const firstKey = keys[0]
    const prevFirstKey = prevKeys[0]
    if (
      prevFirstKey !== undefined &&
      firstKey !== undefined &&
      firstKey !== prevFirstKey
    ) {
      const prependCount = keys.indexOf(prevFirstKey)
      if (prependCount > 0) {
        setFirstItemIndex((v) => v - prependCount)
      } else {
        const trimCount = prevKeys.indexOf(firstKey)
        if (trimCount > 0) {
          setFirstItemIndex((v) => v + trimCount)
        }
      }
    }
    prevRenderKeysRef.current = keys
  }, [listRenderItems])

  // Jump-to-latest via Virtuoso, flagged as programmatic so the drag-up
  // detector can never mistake the resulting scroll events for a user gesture.
  const pinToLatestProgrammatically = useCallback(() => {
    programmaticScrollRef.current = true
    virtuosoRef.current?.scrollToIndex({ index: 'LAST', align: 'end', behavior: 'auto' })
    if (scrollRafRef.current != null) cancelAnimationFrame(scrollRafRef.current)
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = null
      programmaticScrollRef.current = false
    })
  }, [])

  useLayoutEffect(() => {
    if (!initialPinPendingRef.current) return
    if (listRenderItems.length === 0) return
    initialPinPendingRef.current = false
    initialPinDeadlineRef.current = Date.now() + 1500
    followRef.current = true
    atBottomRef.current = true
    setShowScrollToBottom(false)
    requestAnimationFrame(() => {
      pinToLatestProgrammatically()
    })
  }, [listRenderItems.length, pinToLatestProgrammatically])

  const rewindIndexByMsgId = useMemo(() => {
    const map = new Map<string, number>()
    let counter = -1
    for (const item of listRenderItems) {
      if (item.kind !== 'message') continue
      const msg = item.message
      if (msg.type === 'user_text' && !msg.pending && !msg.superseded) {
        const idx =
          typeof msg.userMessageIndex === 'number' ? msg.userMessageIndex : ++counter
        map.set(msg.id, idx)
      }
    }
    return map
  }, [listRenderItems])

  const restoreAnchorMsgId = useMemo(() => {
    if (!pendingRewind) return null
    for (const item of listRenderItems) {
      if (item.kind !== 'message') continue
      const msg = item.message
      if (msg.type === 'user_text' && msg.superseded) {
        return msg.id
      }
    }
    return null
  }, [listRenderItems, pendingRewind])

  const assistantTurnCopyByMsgId = useMemo(
    () => buildAssistantTurnCopyMap(messages),
    [messages],
  )

  const subagentTimelines = useChatStore(
    useShallow((s) =>
      resolvedSessionId
        ? s.sessions[resolvedSessionId]?.subagentTimelines ?? EMPTY_SUBAGENT_TIMELINES
        : EMPTY_SUBAGENT_TIMELINES,
    ),
  )

  const isTailRendering = useMemo(() => {
    if (streamingText.trim().length > 0) return true
    if (activeThinkingId) return true
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (!m) continue
      if (m.type === 'tool_result') continue
      if (m.type === 'thinking') return m.id === activeThinkingId
      if (m.type === 'tool_use') return !toolResultMap.has(m.toolUseId)

      return false
    }
    return false
  }, [messages, streamingText, activeThinkingId, toolResultMap])

  const showPlanningIndicator =
    chatState !== 'idle' &&
    chatState !== 'permission_pending' &&
    !pendingPermission &&
    !isTailRendering

  const closeRewindModal = useCallback(() => {
    if (isExecutingRewind) return
    setRewindTarget(null)
    setRewindPreview(null)
    setRewindError(null)
    setIsLoadingPreview(false)
  }, [isExecutingRewind])

  const handleConfirmRewind = useCallback(async (revertFiles: boolean) => {
    if (!resolvedSessionId || !rewindTarget || isExecutingRewind) return

    setIsExecutingRewind(true)
    setExecutingRewindChoice(revertFiles ? 'revert' : 'no-revert')
    setRewindError(null)

    try {
      if (chatState !== 'idle') {
        stopGeneration(resolvedSessionId)
      }

      const result = await sessionsApi.rewind(resolvedSessionId, {
        userMessageIndex: rewindTarget.userMessageIndex,
        revertFiles,
      })

      const isInlineEditPath = rewindTarget.pendingNewContent !== undefined

      if (isInlineEditPath) {

        if (result.rewindId) {
          try {
            await sessionsApi.commitRewind(resolvedSessionId, result.rewindId)
          } catch {
            // Commit failed: the rewind checkpoint stays open on the backend.
            // Warn the user rather than silently proceeding to resend, since the
            // session and backend rewind state may now disagree.
            addToast({
              type: 'error',
              message: t('chat.rewindCommitFailed'),
              duration: 5000,
            })
          }
        }

        await reloadHistory(resolvedSessionId)

        sendMessage(
          resolvedSessionId,
          rewindTarget.pendingNewContent ?? '',
          rewindTarget.pendingNewAttachments,
        )

        addToast({
          type: 'success',
          message: result.code.available
            ? t('chat.rewindSuccessWithCode', {
                count: result.conversation.messagesRemoved,
              })
            : t('chat.rewindSuccessConversationOnly', {
                count: result.conversation.messagesRemoved,
              }),
        })

        setEditingMessage(null)
      } else {

        await reloadHistory(resolvedSessionId)
        queueComposerPrefill(resolvedSessionId, {
          text: rewindTarget.content,
          attachments: rewindTarget.attachments,
        })

        addToast({
          type: 'success',
          message: result.code.available
            ? t('chat.rewindSuccessWithCode', {
                count: result.conversation.messagesRemoved,
              })
            : t('chat.rewindSuccessConversationOnly', {
                count: result.conversation.messagesRemoved,
              }),
        })
      }

      setRewindTarget(null)
      setRewindPreview(null)
    } catch (error) {
      const message =
        error instanceof ApiError
          ? typeof error.body === 'object' && error.body && 'message' in error.body
            ? String((error.body as { message: unknown }).message)
            : error.message
          : error instanceof Error
            ? error.message
            : String(error)
      setRewindError(message)
    } finally {
      setIsExecutingRewind(false)
      setExecutingRewindChoice(null)
    }
  }, [
    addToast,
    chatState,
    isExecutingRewind,
    queueComposerPrefill,
    reloadHistory,
    resolvedSessionId,
    rewindTarget,
    sendMessage,
    stopGeneration,
    t,
  ])

  const footerContext = useMemo<ListFooterContext>(
    () => ({
      streamingText,
      isStreaming: chatState === 'streaming',
      resolvedSessionId: resolvedSessionId ?? null,
      showRetryBanner,
      showPlanningIndicator,
      awaitingWorkers: chatState === 'awaiting_workers',
      planningLabel:
        t('chat.willResumeWhenWorkersFinish') || 'Will resume when subagents finish',
      activeThinkingId,
      onLiveThinkingGrow: handleLiveThinkingGrow,
    }),
    [
      streamingText,
      chatState,
      resolvedSessionId,
      showRetryBanner,
      showPlanningIndicator,
      t,
      activeThinkingId,
      handleLiveThinkingGrow,
    ],
  )

  const handleRequestRewindCb = useCallback(
    (
      message: Extract<UIMessage, { type: 'user_text' }>,
      userMessageIndex: number,
    ) => {
      setRewindTarget({
        userMessageIndex,
        content: message.content,
        attachments: message.attachments,
      })
    },
    [],
  )
  const handleRequestRestoreCb = useCallback(() => {
    setRestoreConfirmOpen(true)
  }, [])
  const handleEditAsDraftCb = useCallback(
    (
      message: Extract<UIMessage, { type: 'user_text' }>,
      userMessageIndex: number,
    ) => {
      setEditingMessage({
        messageId: message.id,
        userMessageIndex,
        content: message.content,
        attachments: message.attachments,
      })
    },
    [],
  )

  const onRequestRewindHandler = !isMemberSession ? handleRequestRewindCb : undefined
  const onRequestRestoreHandler = !isMemberSession ? handleRequestRestoreCb : undefined
  const onEditAsDraftHandler =
    !isMemberSession && resolvedSessionId ? handleEditAsDraftCb : undefined

  const renderListItem = (item: RenderItem) => {
          if (item.kind === 'explored') {
            const stillStreaming =
              chatState !== 'idle' &&
              item.items.some((entry) => {
                if (entry.type === 'thinking') return entry.id === activeThinkingId
                if (entry.type === 'tool_use') return !toolResultMap.has(entry.toolUseId)
                return false
              })
            const exploredKey = item.items[0]?.id ?? 'explored'
            return (
              <SectionErrorBoundary
                key={exploredKey}
                label="explored"
                resetKeys={[exploredKey]}
              >
                <ExploredCard
                  items={item.items}
                  resultMap={toolResultMap}
                  summary={item.summary}
                  isStreaming={stillStreaming}
                  activeThinkingId={activeThinkingId}
                  sessionId={resolvedSessionId}
                  onLiveThinkingGrow={handleLiveThinkingGrow}
                />
              </SectionErrorBoundary>
            )
          }

          const msg = item.message

          const rewindableUserIndex: number | null =
            msg.type === 'user_text' ? rewindIndexByMsgId.get(msg.id) ?? null : null
          const isRestoreAnchor =
            !!restoreAnchorMsgId &&
            msg.type === 'user_text' &&
            msg.id === restoreAnchorMsgId

          if (
            msg.type === 'user_text' &&
            editingMessage &&
            editingMessage.messageId === msg.id &&
            resolvedSessionId
          ) {
            return (
              <InlineUserMessageEditor
                key={msg.id}
                sessionId={resolvedSessionId}
                initialContent={editingMessage.content}
                initialAttachments={editingMessage.attachments}
                onCancel={() => setEditingMessage(null)}
                onSubmit={(newContent, newAttachments) => {
                  if (!editingMessage) return
                  setRewindTarget({
                    userMessageIndex: editingMessage.userMessageIndex,
                    content: editingMessage.content,
                    attachments: editingMessage.attachments,
                    pendingNewContent: newContent,
                    pendingNewAttachments: newAttachments,
                  })
                }}
              />
            )
          }

          if (
            msg.type === 'subagent_chunk' &&
            msg.parentToolUseId &&
            subagentTimelines[msg.parentToolUseId]
          ) {
            return null
          }
          const turnCopy =
            msg.type === 'assistant_text'
              ? assistantTurnCopyByMsgId.get(msg.id) ?? null
              : null

          const resolvedToolResult =
            msg.type === 'tool_use'
              ? (() => {
                  const r = toolResultMap.get(msg.toolUseId)
                  return r ? { content: r.content, isError: r.isError } : null
                })()
              : null
          // Narrow chatState to per-block booleans that stay constant for completed blocks:
          // only the in-flight tool (no result yet) and the last assistant segment can change,
          // so streaming<->tool transitions no longer invalidate the whole history.
          const toolStreaming =
            msg.type === 'tool_use' &&
            chatState === 'tool_executing' &&
            resolvedToolResult == null
          const tailMenuEnabled =
            msg.type === 'assistant_text' &&
            chatState === 'idle' &&
            Boolean(turnCopy?.isLastAssistantSegmentInTurn) &&
            Boolean((turnCopy?.fullText ?? '').trim())

          const block = (
            <SectionErrorBoundary key={msg.id} label="message" resetKeys={[msg.id]}>
            <MessageBlock
              message={msg}
              activeThinkingId={activeThinkingId}
              toolStreaming={toolStreaming}
              tailMenuEnabled={tailMenuEnabled}
              assistantTurnCopy={turnCopy}
              sessionId={resolvedSessionId}
              sessionWorkDir={activeSessionMeta?.workDir ?? null}
              disableFork={isMemberSession}
              childCalls={
                msg.type === 'tool_use'
                  ? childToolCallsByParent.get(msg.toolUseId) ?? undefined
                  : undefined
              }
              childResults={
                msg.type === 'tool_use'
                  ? childResultsByParent.get(msg.toolUseId) ?? undefined
                  : undefined
              }
              toolResult={resolvedToolResult}
              rewindableUserIndex={rewindableUserIndex}
              isRestoreAnchor={isRestoreAnchor}
              onRequestRewind={onRequestRewindHandler}
              onRequestRestore={onRequestRestoreHandler}
              onEditAsDraft={
                typeof rewindableUserIndex === 'number'
                  ? onEditAsDraftHandler
                  : undefined
              }
              onLiveThinkingGrow={handleLiveThinkingGrow}
            />
            </SectionErrorBoundary>
          )

          return block
  }

  return (
    <div className="relative flex flex-1 min-h-0 flex-col">
      <Virtuoso<RenderItem, ListFooterContext>
        key={resolvedSessionId ?? '__none__'}
        ref={virtuosoRef}
        scrollerRef={handleScrollerRef}
        className="flex-1 message-list-scroll"
        data={listRenderItems}
        context={footerContext}
        computeItemKey={(_, item) => renderItemKey(item)}
        firstItemIndex={firstItemIndex}
        initialTopMostItemIndex={Math.max(0, listRenderItems.length - 1)}
        followOutput={false}
        totalListHeightChanged={() => {
          if (!followRef.current) return
          if (typeof document !== 'undefined' && document.hidden) return
          if (Date.now() < initialPinDeadlineRef.current) {
            pinToLatestProgrammatically()
            return
          }
          // At idle, height growth right after the user clicked inside the
          // list (expanding a tool card, opening a diff) is their own doing:
          // snapping to the bottom would yank away what they opened. During a
          // live turn we always follow.
          if (
            chatState === 'idle' &&
            Date.now() - lastScrollerPointerDownAtRef.current < IDLE_INTERACTION_GRACE_MS
          ) {
            return
          }
          scrollFollowToBottom()
        }}
        atBottomStateChange={(atBottom) => {
          atBottomRef.current = atBottom
          if (atBottom) {
            followRef.current = true
            setShowScrollToBottom(false)
          } else if (!followRef.current) {
            setShowScrollToBottom(true)
          }
        }}
        atTopStateChange={(atTop) => {
          atTopRef.current = atTop
          if (atTop) maybeLoadOlder()
        }}
        atBottomThreshold={AUTO_SCROLL_BOTTOM_THRESHOLD_PX}
        increaseViewportBy={{ top: 800, bottom: 1200 }}
        startReached={maybeLoadOlder}
        components={VIRTUOSO_COMPONENTS}
        itemContent={(_, item) => (
          <div className="mx-auto w-full max-w-[860px] flow-root px-4">
            {renderListItem(item)}
          </div>
        )}
      />

      {}
      <Modal
        open={Boolean(rewindTarget)}
        onClose={closeRewindModal}
        title={t('chat.rewindModalTitleSubmit')}

        width={580}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={closeRewindModal}
              disabled={isExecutingRewind}
            >
              {t('common.cancel')}
            </Button>
            <Button
              variant="secondary"
              className="whitespace-nowrap"
              onClick={() => {
                void handleConfirmRewind(false)
              }}
              loading={executingRewindChoice === 'no-revert'}
              disabled={
                isLoadingPreview ||
                Boolean(rewindError) ||
                (isExecutingRewind && executingRewindChoice !== 'no-revert')
              }
            >
              {t('chat.rewindContinueOnly')}
            </Button>
            <Button
              variant="primary"
              className="whitespace-nowrap"
              onClick={() => {
                void handleConfirmRewind(true)
              }}
              loading={executingRewindChoice === 'revert'}
              disabled={
                isLoadingPreview ||
                Boolean(rewindError) ||
                (isExecutingRewind && executingRewindChoice !== 'revert')
              }
              icon={
                executingRewindChoice !== 'revert' ? (
                  <span className="material-symbols-outlined text-[16px]">undo</span>
                ) : undefined
              }
            >
              {t('chat.rewindContinueAndRevert')}
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          {isLoadingPreview && !rewindPreview ? (
            <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
              {t('chat.rewindLoading')}
            </p>
          ) : (
            <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
              {(() => {
                const messageCount = rewindPreview?.conversation.messagesRemoved ?? 0
                const fileCount = rewindPreview?.code.filesChanged.length ?? 0
                return fileCount > 0
                  ? t('chat.rewindModalBody', { fileCount, messageCount })
                  : t('chat.rewindModalBodyNoFiles', { messageCount })
              })()}
            </p>
          )}

          {rewindError && (
            <div className="rounded-[var(--radius-md)] border border-[var(--color-error)]/30 bg-[var(--color-error-container)]/22 px-3 py-2 text-xs text-[var(--color-error)]">
              {rewindError}
            </div>
          )}
        </div>
      </Modal>

      <Modal
        open={restoreConfirmOpen}
        onClose={() => {
          if (!isRestoring) setRestoreConfirmOpen(false)
        }}
        title={t('chat.restoreConfirmTitle')}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => setRestoreConfirmOpen(false)}
              disabled={isRestoring}
            >
              {t('common.cancel')}
            </Button>
            <Button
              loading={isRestoring}
              onClick={() => {
                if (!resolvedSessionId) return
                setIsRestoring(true)
                void restoreRewindAction(resolvedSessionId)
                  .then(() => {
                    setRestoreConfirmOpen(false)
                  })
                  .finally(() => {
                    setIsRestoring(false)
                  })
              }}
              icon={
                !isRestoring ? (
                  <span className="material-symbols-outlined text-[16px]">restore</span>
                ) : undefined
              }
            >
              {t('chat.restoreConfirmConfirm')}
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {(() => {
              const fileCount = pendingRewind?.filesChanged.length ?? 0
              return fileCount > 0
                ? t('chat.restoreConfirmBody', { fileCount })
                : t('chat.restoreConfirmBodyNoFiles')
            })()}
          </p>
          {pendingRewind && pendingRewind.filesChanged.length > 0 && (
            <div className="rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-4 py-3">
              <div className="flex flex-wrap gap-2">
                {pendingRewind.filesChanged.slice(0, 8).map((p) => (
                  <span
                    key={p}
                    className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1 text-[11px] text-[var(--color-text-secondary)]"
                  >
                    {p}
                  </span>
                ))}
                {pendingRewind.filesChanged.length > 8 && (
                  <span className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1 text-[11px] text-[var(--color-text-secondary)]">
                    {t('chat.rewindFilesMore', {
                      count: pendingRewind.filesChanged.length - 8,
                    })}
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
      </Modal>

      <Modal
        open={Boolean(pendingSendAfterRewind)}
        onClose={() => {
          if (!isCommittingSendAfterRewind && resolvedSessionId) {
            cancelSendAfterRewind(resolvedSessionId)
          }
        }}
        title={t('chat.sendAfterRewindTitle')}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => {
                if (resolvedSessionId) cancelSendAfterRewind(resolvedSessionId)
              }}
              disabled={isCommittingSendAfterRewind}
            >
              {t('common.cancel')}
            </Button>
            <Button
              loading={isCommittingSendAfterRewind}
              onClick={() => {
                if (!resolvedSessionId) return
                setIsCommittingSendAfterRewind(true)
                void confirmSendAfterRewind(resolvedSessionId).finally(() => {
                  setIsCommittingSendAfterRewind(false)
                })
              }}
              icon={
                !isCommittingSendAfterRewind ? (
                  <span className="material-symbols-outlined text-[16px]">send</span>
                ) : undefined
              }
            >
              {t('chat.sendAfterRewindConfirm')}
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {(() => {
              const fileCount = pendingRewind?.filesChanged.length ?? 0
              return fileCount > 0
                ? t('chat.sendAfterRewindBody', { fileCount })
                : t('chat.sendAfterRewindBodyNoFiles')
            })()}
          </p>
          {pendingRewind && pendingRewind.filesChanged.length > 0 && (
            <div className="rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-4 py-3">
              <div className="flex flex-wrap gap-2">
                {pendingRewind.filesChanged.slice(0, 8).map((p) => (
                  <span
                    key={p}
                    className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1 text-[11px] text-[var(--color-text-secondary)]"
                  >
                    {p}
                  </span>
                ))}
                {pendingRewind.filesChanged.length > 8 && (
                  <span className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1 text-[11px] text-[var(--color-text-secondary)]">
                    {t('chat.rewindFilesMore', {
                      count: pendingRewind.filesChanged.length - 8,
                    })}
                  </span>
                )}
              </div>
            </div>
          )}
        </div>
      </Modal>
    {showScrollToBottom && (
      <button
        type="button"
        onClick={scrollToBottomNow}
        title={t('chat.scrollToLatestTooltip')}
        aria-label={t('chat.scrollToLatest')}
        className="absolute bottom-4 right-6 z-20 flex h-9 items-center gap-1.5 rounded-full border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-xs font-medium text-[var(--color-text-primary)] shadow-md hover:bg-[var(--color-surface-hover)] hover:shadow-lg active:scale-95 transition-[background-color,box-shadow,transform] duration-150"
        style={{ backdropFilter: 'blur(6px)' }}
      >
        <span className="material-symbols-outlined text-[16px]">arrow_downward</span>
        <span>{t('chat.scrollToLatest')}</span>
      </button>
    )}
    </div>
  )
}

type MessageBlockProps = {
  message: UIMessage
  activeThinkingId: string | null
  toolResult?: { content: unknown; isError: boolean } | null
  // Narrowed, history-stable flags derived from chatState in the parent. Passing the raw
  // chatState into every block made all memoized history blocks invalidate on every
  // streaming<->tool_executing transition (flicker / layout jump during active editing).
  // These booleans are constant for completed blocks, so only the active block re-renders.
  toolStreaming: boolean
  tailMenuEnabled: boolean
  assistantTurnCopy: AssistantTurnCopyInfo | null
  sessionId?: string | null
  sessionWorkDir?: string | null
  disableFork?: boolean
  rewindableUserIndex?: number | null
  isRestoreAnchor?: boolean
  onRequestRewind?: (
    message: Extract<UIMessage, { type: 'user_text' }>,
    userMessageIndex: number,
  ) => void
  onRequestRestore?: () => void
  onEditAsDraft?: (
    message: Extract<UIMessage, { type: 'user_text' }>,
    userMessageIndex: number,
  ) => void
  childCalls?: Extract<UIMessage, { type: 'tool_use' }>[]
  childResults?: Map<string, Extract<UIMessage, { type: 'tool_result' }>>
  onLiveThinkingGrow?: () => void
}

function areMessageBlockPropsEqual(
  prev: MessageBlockProps,
  next: MessageBlockProps,
): boolean {
  if (prev.message !== next.message) return false
  if (prev.activeThinkingId !== next.activeThinkingId) return false
  if (prev.toolStreaming !== next.toolStreaming) return false
  if (prev.tailMenuEnabled !== next.tailMenuEnabled) return false
  if (prev.sessionId !== next.sessionId) return false
  if (prev.sessionWorkDir !== next.sessionWorkDir) return false
  if (prev.disableFork !== next.disableFork) return false
  if (prev.rewindableUserIndex !== next.rewindableUserIndex) return false
  if (prev.isRestoreAnchor !== next.isRestoreAnchor) return false
  if (prev.toolResult !== next.toolResult) {
    if (!prev.toolResult || !next.toolResult) return false
    if (
      prev.toolResult.content !== next.toolResult.content ||
      prev.toolResult.isError !== next.toolResult.isError
    )
      return false
  }
  if (prev.assistantTurnCopy !== next.assistantTurnCopy) {
    if (
      !prev.assistantTurnCopy ||
      !next.assistantTurnCopy ||
      prev.assistantTurnCopy.fullText !== next.assistantTurnCopy.fullText ||
      prev.assistantTurnCopy.isLastAssistantSegmentInTurn !==
        next.assistantTurnCopy.isLastAssistantSegmentInTurn
    ) {
      return false
    }
  }
  if (prev.childCalls !== next.childCalls) return false
  if (prev.childResults !== next.childResults) return false
  if (prev.onRequestRewind !== next.onRequestRewind) return false
  if (prev.onRequestRestore !== next.onRequestRestore) return false
  if (prev.onEditAsDraft !== next.onEditAsDraft) return false
  return true
}

export const MessageBlock = memo(function MessageBlock({
  message,
  activeThinkingId,
  toolResult,
  toolStreaming,
  tailMenuEnabled,
  assistantTurnCopy,
  sessionId,
  sessionWorkDir,
  disableFork,
  rewindableUserIndex,
  isRestoreAnchor,
  onRequestRewind,
  onRequestRestore,
  onEditAsDraft,
  childCalls,
  childResults,
  onLiveThinkingGrow,
}: MessageBlockProps) {
  const t = useTranslation()

  const supersededWrap = (node: React.ReactNode) =>
    message.superseded ? (
      <div className="opacity-60 saturate-50 pointer-events-none select-none">
        {node}
      </div>
    ) : (
      node
    )

  switch (message.type) {
    case 'user_text':
      return (
        <UserMessage
          content={message.content}
          attachments={message.attachments}
          designRef={message.designRef}
          designRefName={message.designRefName}
          designRefElement={message.designRefElement}
          designRefElementLabel={message.designRefElementLabel}
          onRewind={
            !isRestoreAnchor &&
            typeof rewindableUserIndex === 'number' &&
            onRequestRewind &&
            !message.superseded
              ? () => onRequestRewind(message, rewindableUserIndex)
              : undefined
          }
          onRestore={isRestoreAnchor && onRequestRestore ? onRequestRestore : undefined}
          rewindLabel={t('chat.rewindActionShort')}
          restoreLabel={t('chat.restoreLastConversation')}
          onEditAsDraft={
            onEditAsDraft &&
            !message.pending &&
            typeof rewindableUserIndex === 'number'
              ? () => onEditAsDraft(message, rewindableUserIndex)
              : undefined
          }
          superseded={message.superseded}
        />
      )
    case 'assistant_text': {
      const fullTurn = assistantTurnCopy?.fullText ?? ''
      const allowTailMenu = tailMenuEnabled
      return supersededWrap(
        <AssistantMessage
          content={message.content}
          assistantTurnCopyText={allowTailMenu ? fullTurn : undefined}
          sessionId={sessionId}
          workDir={sessionWorkDir}
          disableFork={disableFork}
        />
      )
    }
    case 'thinking':
      if (message.id === activeThinkingId) {
        return supersededWrap(
          <ActiveThinkingBlock
            sessionId={sessionId ?? null}
            onContentGrow={onLiveThinkingGrow}
          />
        )
      }
      return supersededWrap(
        <ThinkingBlock
          content={message.content}
          isActive={false}
          startedAt={message.startedAt}
          completedAt={message.completedAt}
        />
      )
    case 'tool_use': {
      if (isAskQuestionToolName(message.toolName)) {
        return supersededWrap(
          <AskUserQuestion
            toolUseId={message.toolUseId}
            input={message.input}
            result={toolResult?.content}
            sessionId={sessionId}
          />
        )
      }
      if (
        message.toolName === 'exit_curator_mode' &&
        toolResult &&
        !toolResult.isError
      ) {
        const resultText =
          typeof toolResult.content === 'string'
            ? toolResult.content
            : Array.isArray(toolResult.content)
              ? toolResult.content
                  .filter((c: any) => c && typeof c.text === 'string')
                  .map((c: any) => c.text)
                  .join('\n')
              : ''
        if (resultText.includes('===CURATOR_MARKDOWN_BEGIN===')) {
          const parsed = parseCuratorEnvelope(resultText)
          if (parsed) {
            return supersededWrap(
              <CuratorCard
                messageId={message.id}
                slug={parsed.slug}
                template={parsed.template}
                finalMdPath={parsed.finalMdPath}
                implBlueprintPath={parsed.implBlueprintPath}
                docxPath={parsed.docxPath}
                title={parsed.title}
                body={parsed.body}
                sessionId={sessionId}
              />
            )
          }
        }
      }
      return supersededWrap(
        <ToolCard
          toolName={message.toolName}
          toolUseId={message.toolUseId}
          input={message.input}
          result={toolResult ?? null}
          isStreaming={toolStreaming}
          parentSessionId={sessionId}
          toolTimestamp={message.timestamp}
          childCalls={childCalls}
          childResults={childResults}
        />
      )
    }
    case 'tool_result': {
      const resultText =
        typeof message.content === 'string'
          ? message.content
          : Array.isArray(message.content)
            ? message.content
                .filter((c: any) => c && typeof c.text === 'string')
                .map((c: any) => c.text)
                .join('\n')
            : ''
      if (!message.isError && resultText.includes('===CURATOR_MARKDOWN_BEGIN===')) {
        const parsed = parseCuratorEnvelope(resultText)
        if (parsed) {
          return supersededWrap(
            <CuratorCard
              messageId={message.id}
              slug={parsed.slug}
              template={parsed.template}
              finalMdPath={parsed.finalMdPath}
              implBlueprintPath={parsed.implBlueprintPath}
              docxPath={parsed.docxPath}
              title={parsed.title}
              body={parsed.body}
              sessionId={sessionId}
            />
          )
        }
      }
      return supersededWrap(
        <ToolResultBlock
          content={message.content}
          isError={message.isError}
          standalone
        />
      )
    }
    case 'permission_request':
      return supersededWrap(
        <PermissionDialog
          requestId={message.requestId}
          toolName={message.toolName}
          input={message.input}
          description={message.description}
          sessionId={sessionId}
        />
      )
    case 'error': {
      const { friendly, technicalDetail } = resolveErrorDisplay(message, t)
      return supersededWrap(
        <div className="mb-2 px-4 py-2 rounded-lg border border-[var(--color-error)]/20 bg-[var(--color-error-container)]/28 text-sm text-[var(--color-error)]">
          <strong>Error:</strong> {friendly}
          {technicalDetail && (
            <details className="mt-1 group">
              <summary className="cursor-pointer text-xs text-[var(--color-on-error-container)]/75 hover:text-[var(--color-on-error-container)] select-none">
                {t('chat.errorDetails')}
              </summary>
              <pre className="mt-1 whitespace-pre-wrap break-words font-[var(--font-mono)] text-[11px] leading-[1.5] text-[var(--color-on-error-container)]/85 max-h-64 overflow-y-auto">
                {technicalDetail}
              </pre>
            </details>
          )}
        </div>
      )
    }
    case 'task_summary':
      return supersededWrap(<InlineTaskSummary tasks={message.tasks} />)
    case 'system': {
      const systemContent = message.content ?? ''
      if (systemContent.startsWith('[Pair Checkpoint]')) {
        return supersededWrap(
          <PairCheckpointCard superseded={message.superseded} />
        )
      }
      return supersededWrap(
        <div className="mb-2 text-center text-xs text-[var(--color-text-tertiary)]">
          {systemContent}
        </div>
      )
    }
    case 'file_edit':
      return supersededWrap(
        <FileEditNotification
          path={message.path}
          additions={message.additions}
          deletions={message.deletions}
          diff={message.diff}
          editBatchId={message.editBatchId}
        />
      )
    case 'command_preview':
      return supersededWrap(
        <CommandPreviewCard
          toolName={message.toolName}
          input={message.input}
        />
      )
    case 'subagent_chunk':
      return supersededWrap(
        <SubagentChunkBlock
          agentId={message.agentId}
          delta={message.delta}
          chunkKind={message.chunkKind}
          taskId={message.taskId}
        />
      )
    case 'plan_question_answers':
      return (
        <AnswersCard
          items={message.items}
          details={message.details}
          superseded={message.superseded}
        />
      )
    case 'plan_card':
      return (
        <PlanCard
          messageId={message.id}
          planPath={message.planPath}
          fileName={message.fileName}
          title={message.title}
          overview={message.overview}
          todos={message.todos}
          markdown={message.markdown}
          modelLabel={message.modelLabel}
          status={message.status}
          superseded={message.superseded}
          sessionId={sessionId}
        />
      )
    case 'mode_switch_card':
      return (
        <ModeSwitchCard
          messageId={message.id}
          planPath={message.planPath}
          targetMode={message.targetMode}
          status={message.status}
          superseded={message.superseded}
          sessionId={sessionId}
          handoffKind={message.handoffKind}
        />
      )
    case 'plan_progress':
      return (
        <PlanProgressCard
          planPath={message.planPath}
          title={message.title}
          todos={message.todos}
          superseded={message.superseded}
          handoffKind={message.handoffKind}
        />
      )
    case 'curator_card':
      return supersededWrap(
        <CuratorCard
          messageId={message.id}
          slug={message.slug}
          template={message.template}
          finalMdPath={message.finalMdPath}
          implBlueprintPath={message.implBlueprintPath}
          docxPath={message.docxPath}
          title={message.title}
          body={message.body}
          status={message.status}
          error={message.error}
          sessionId={sessionId}
        />
      )
    case 'plan_mode_blocked':
      return (
        <PlanModeBlockedNotice
          tools={message.tools}
          superseded={message.superseded}
          mode={(message.mode as never) ?? 'plan'}
          reason={message.reason ?? 'plan'}
          detail={message.detail}
        />
      )
  }
}, areMessageBlockPropsEqual)
