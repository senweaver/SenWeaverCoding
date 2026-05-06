import { useRef, useEffect, useMemo, memo, useState, useCallback } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { ApiError } from '../../api/client'
import { sessionsApi, type SessionRewindResponse } from '../../api/sessions'
import { useChatStore, isAskQuestionToolName } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useTeamStore } from '../../stores/teamStore'
import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'
import type { TranslationKey } from '../../i18n/locales/en'
import { UserMessage } from './UserMessage'
import { InlineUserMessageEditor } from './InlineUserMessageEditor'
import { AssistantMessage } from './AssistantMessage'
import { ThinkingBlock } from './ThinkingBlock'
import { ToolCard } from './tools/ToolCard'
import { ExploredCard, buildExploredSummary, type ExploredSummary } from './ExploredCard'
import { ToolResultBlock } from './ToolResultBlock'
import { PermissionDialog } from './PermissionDialog'
import { FileEditNotification } from './FileEditNotification'
import { CommandPreviewCard } from './CommandPreviewCard'
import { SubagentChunkBlock } from './SubagentChunkBlock'
import { AskUserQuestion } from './AskUserQuestion'
import { StreamingIndicator } from './StreamingIndicator'
import { InlineTaskSummary } from './InlineTaskSummary'
import { AnswersCard } from './AnswersCard'
import { PlanCard } from './PlanCard'
import { ModeSwitchCard } from './ModeSwitchCard'
import { PairCheckpointCard } from './PairCheckpointCard'
import { PlanModeBlockedNotice } from './PlanModeBlockedNotice'
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

type RenderItem =
  | { kind: 'explored'; id: string; items: UIMessage[]; summary: ExploredSummary }
  | { kind: 'message'; message: UIMessage }

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
  let nextExploredId = 0

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
      items.push({
        kind: 'explored',
        id: `explored-${++nextExploredId}-${buffer[0]?.id ?? ''}`,
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
    }
    if (activePlanIdx >= 0 && activePlanIdx < items.length - 1) {
      const planItem = items[activePlanIdx]!
      items.splice(activePlanIdx, 1)
      items.push(planItem)
    }
  }

  return { renderItems: items, toolResultMap, childToolCallsByParent }
}

type MessageListProps = {
  sessionId?: string | null
}

const AUTO_SCROLL_BOTTOM_THRESHOLD_PX = 96
const AUTO_SCROLL_RESUME_THRESHOLD_PX = 220

const EMPTY_MESSAGES: UIMessage[] = []

function isNearScrollBottom(element: HTMLElement) {
  return (
    element.scrollHeight - element.scrollTop - element.clientHeight <=
    AUTO_SCROLL_BOTTOM_THRESHOLD_PX
  )
}

function isWithinResumeRange(element: HTMLElement) {
  return (
    element.scrollHeight - element.scrollTop - element.clientHeight <=
    AUTO_SCROLL_RESUME_THRESHOLD_PX
  )
}

export function MessageList({ sessionId }: MessageListProps = {}) {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const resolvedSessionId = sessionId ?? activeTabId
  const messages = useChatStore(
    (s) =>
      (resolvedSessionId ? s.sessions[resolvedSessionId]?.messages : undefined) ??
      EMPTY_MESSAGES,
  )
  const chatState = useChatStore((s) =>
    resolvedSessionId ? s.sessions[resolvedSessionId]?.chatState ?? 'idle' : 'idle',
  )
  const streamingText = useChatStore((s) =>
    resolvedSessionId ? s.sessions[resolvedSessionId]?.streamingText ?? '' : '',
  )
  const activeThinkingId = useChatStore((s) =>
    resolvedSessionId
      ? s.sessions[resolvedSessionId]?.activeThinkingId ?? null
      : null,
  )
  const activeThinkingContent = useChatStore((s) =>
    resolvedSessionId
      ? s.sessions[resolvedSessionId]?.activeThinkingContent ?? ''
      : '',
  )
  const pendingPermission = useChatStore((s) =>
    resolvedSessionId
      ? s.sessions[resolvedSessionId]?.pendingPermission ?? null
      : null,
  )
  const pendingComputerUsePermission = useChatStore((s) =>
    resolvedSessionId
      ? s.sessions[resolvedSessionId]?.pendingComputerUsePermission ?? null
      : null,
  )
  const pendingRewind = useChatStore((s) =>
    resolvedSessionId ? s.sessions[resolvedSessionId]?.pendingRewind ?? null : null,
  )
  const pendingSendAfterRewind = useChatStore((s) =>
    resolvedSessionId
      ? s.sessions[resolvedSessionId]?.pendingSendAfterRewind ?? null
      : null,
  )
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
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const shouldAutoScrollRef = useRef(true)
  const lastSessionIdRef = useRef<string | null | undefined>(resolvedSessionId)
  const scrollRafRef = useRef<number | null>(null)
  const pendingScrollRef = useRef(false)
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

  const updateAutoScrollState = useCallback(() => {
    const container = scrollContainerRef.current
    if (!container) return
    const near = isNearScrollBottom(container)
    const withinResume = isWithinResumeRange(container)
    shouldAutoScrollRef.current = near || (shouldAutoScrollRef.current && withinResume)
    const showBtn = !shouldAutoScrollRef.current
    setShowScrollToBottom((prev) => (prev !== showBtn ? showBtn : prev))
  }, [])

  const scheduleAutoScroll = useCallback(() => {
    if (!shouldAutoScrollRef.current) return
    if (typeof document !== 'undefined' && document.hidden) {

      pendingScrollRef.current = true
      return
    }
    if (scrollRafRef.current !== null) return
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = null
      pendingScrollRef.current = false
      const container = scrollContainerRef.current
      if (!container) return
      if (typeof document !== 'undefined' && document.hidden) {
        pendingScrollRef.current = true
        return
      }
      if (!shouldAutoScrollRef.current) return

      container.scrollTop = container.scrollHeight
    })
  }, [])

  const scrollToBottomNow = useCallback(() => {
    const container = scrollContainerRef.current
    if (!container) return
    shouldAutoScrollRef.current = true
    pendingScrollRef.current = false
    container.scrollTop = container.scrollHeight
    setShowScrollToBottom(false)
  }, [])

  useEffect(() => {
    if (lastSessionIdRef.current !== resolvedSessionId) {
      shouldAutoScrollRef.current = true
      lastSessionIdRef.current = resolvedSessionId
      setShowScrollToBottom(false)
    } else {
      const container = scrollContainerRef.current
      if (container && !shouldAutoScrollRef.current && isWithinResumeRange(container)) {
        shouldAutoScrollRef.current = true
        setShowScrollToBottom(false)
      }
    }
    scheduleAutoScroll()
  }, [messages.length, resolvedSessionId, streamingText, scheduleAutoScroll])

  useEffect(() => {
    const container = scrollContainerRef.current
    if (!container) return
    const inner = container.firstElementChild as HTMLElement | null
    if (!inner) return
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => {
      const c = scrollContainerRef.current
      if (!c) return
      if (shouldAutoScrollRef.current) {
        scheduleAutoScroll()
      } else {
        const near = isNearScrollBottom(c)
        setShowScrollToBottom((prev) => (prev !== !near ? !near : prev))
      }
    })
    ro.observe(inner)
    return () => ro.disconnect()
  }, [scheduleAutoScroll])

  useEffect(() => {
    if (typeof document === 'undefined') return
    const onVisibility = () => {
      if (document.hidden) return
      if (!(pendingScrollRef.current || shouldAutoScrollRef.current)) return
      pendingScrollRef.current = false
      requestAnimationFrame(() => {
        if (document.hidden) {
          pendingScrollRef.current = true
          return
        }
        if (!shouldAutoScrollRef.current) return
        const container = scrollContainerRef.current
        if (container) {
          container.scrollTop = container.scrollHeight
        }
      })
    }
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [])

  useEffect(() => {
    return () => {
      if (scrollRafRef.current !== null) {
        cancelAnimationFrame(scrollRafRef.current)
        scrollRafRef.current = null
      }
    }
  }, [])

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

  const messagesWithLiveThinking = useMemo(() => {
    if (!activeThinkingId) return messages
    if (!activeThinkingContent) return messages
    let foundIdx = -1
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (!m) continue
      if (m.type === 'thinking' && m.id === activeThinkingId) {
        foundIdx = i
        break
      }
    }
    if (foundIdx >= 0) {
      const target = messages[foundIdx] as Extract<UIMessage, { type: 'thinking' }>
      if (target.content === activeThinkingContent) return messages
      const next = messages.slice()
      next[foundIdx] = { ...target, content: activeThinkingContent }
      return next
    }
    const now = Date.now()
    return [
      ...messages,
      {
        id: activeThinkingId,
        type: 'thinking',
        content: activeThinkingContent,
        timestamp: now,
        startedAt: now,
      },
    ] as UIMessage[]
  }, [messages, activeThinkingId, activeThinkingContent])

  const { toolResultMap, renderItems, childToolCallsByParent } = useMemo(
    () => buildRenderModel(messagesWithLiveThinking),
    [messagesWithLiveThinking],
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

  const visibleRenderItems = useMemo(() => {
    if (chatState !== 'idle') return renderItems
    if (streamingText.trim().length > 0) return renderItems
    let end = renderItems.length
    while (end > 0) {
      const item = renderItems[end - 1]!
      if (item.kind === 'message' && item.message.type === 'thinking') {
        end -= 1
      } else {
        break
      }
    }
    return end === renderItems.length ? renderItems : renderItems.slice(0, end)
  }, [renderItems, chatState, streamingText])

  const assistantTurnCopyByMsgId = useMemo(
    () => buildAssistantTurnCopyMap(messages),
    [messages],
  )

  const subagentTimelines = useChatStore(
    useShallow((s) =>
      resolvedSessionId ? s.sessions[resolvedSessionId]?.subagentTimelines ?? {} : {},
    ),
  )

  const isTailRendering = useMemo(() => {
    if (streamingText.trim().length > 0) return true
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
    !pendingComputerUsePermission &&
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

  let visibleUserMessageIndex = -1

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

  return (
    <div className="relative flex flex-1 min-h-0 flex-col">
    <div
      ref={scrollContainerRef}
      onScroll={updateAutoScrollState}
      className="flex-1 overflow-y-auto px-4 py-4 message-list-scroll"
    >
      <div className="mx-auto max-w-[860px]">
        {visibleRenderItems.map((item, index) => {
          const isTailItem = index >= visibleRenderItems.length - 4
          if (item.kind === 'explored') {
            const stillStreaming =
              chatState !== 'idle' &&
              item.items.some((entry) => {
                if (entry.type === 'thinking') return entry.id === activeThinkingId
                if (entry.type === 'tool_use') return !toolResultMap.has(entry.toolUseId)
                return false
              })
            return (
              <div key={item.id} className={isTailItem ? undefined : 'cv-auto'}>
                <ExploredCard
                  items={item.items}
                  resultMap={toolResultMap}
                  summary={item.summary}
                  isStreaming={stillStreaming}
                  activeThinkingId={activeThinkingId}
                />
              </div>
            )
          }

          const msg = item.message

          let rewindableUserIndex: number | null = null
          if (msg.type === 'user_text' && !msg.pending && !msg.superseded) {
            rewindableUserIndex =
              typeof msg.userMessageIndex === 'number'
                ? msg.userMessageIndex
                : ++visibleUserMessageIndex
          }
          const isRestoreAnchor =
            !!pendingRewind &&
            msg.type === 'user_text' &&
            typeof msg.userMessageIndex === 'number' &&
            msg.userMessageIndex === pendingRewind.userMessageIndex

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

          const block = (
            <MessageBlock
              key={msg.id}
              message={msg}
              activeThinkingId={activeThinkingId}
              chatState={chatState}
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
              toolResult={
                msg.type === 'tool_use'
                  ? (() => {
                      const r = toolResultMap.get(msg.toolUseId)
                      return r ? { content: r.content, isError: r.isError } : null
                    })()
                  : null
              }
              rewindableUserIndex={rewindableUserIndex}
              isRestoreAnchor={isRestoreAnchor}
              onRequestRewind={onRequestRewindHandler}
              onRequestRestore={onRequestRestoreHandler}
              onEditAsDraft={
                typeof rewindableUserIndex === 'number'
                  ? onEditAsDraftHandler
                  : undefined
              }
            />
          )

          if (isTailItem) return block
          return (
            <div key={msg.id} className="cv-auto">
              {block}
            </div>
          )
        })}

        {streamingText && (
          <AssistantMessage content={streamingText} isStreaming={chatState === 'streaming'} />
        )}

        {showPlanningIndicator && <StreamingIndicator />}

        <div ref={bottomRef} />
      </div>

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
    </div>
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

type ChatStateLite = 'idle' | 'thinking' | 'tool_executing' | 'streaming' | 'permission_pending'

type MessageBlockProps = {
  message: UIMessage
  activeThinkingId: string | null
  toolResult?: { content: unknown; isError: boolean } | null
  chatState: ChatStateLite
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
}

function areMessageBlockPropsEqual(
  prev: MessageBlockProps,
  next: MessageBlockProps,
): boolean {
  if (prev.message !== next.message) return false
  if (prev.activeThinkingId !== next.activeThinkingId) return false
  if (prev.chatState !== next.chatState) return false
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
  chatState,
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
      const allowTailMenu =
        chatState === 'idle' &&
        Boolean(assistantTurnCopy?.isLastAssistantSegmentInTurn) &&
        Boolean(fullTurn.trim())
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
      return supersededWrap(
        <ThinkingBlock
          content={message.content}
          isActive={message.id === activeThinkingId}
          startedAt={message.startedAt}
          completedAt={message.completedAt}
        />
      )
    case 'tool_use':

      if (isAskQuestionToolName(message.toolName)) {
        return supersededWrap(
          <AskUserQuestion
            toolUseId={message.toolUseId}
            input={message.input}
            result={toolResult?.content}
          />
        )
      }
      return supersededWrap(
        <ToolCard
          toolName={message.toolName}
          toolUseId={message.toolUseId}
          input={message.input}
          result={toolResult ?? null}
          isStreaming={
            chatState === 'tool_executing' && (toolResult == null)
          }
          childCalls={childCalls}
          childResults={childResults}
        />
      )
    case 'tool_result':
      return supersededWrap(
        <ToolResultBlock
          content={message.content}
          isError={message.isError}
          standalone
        />
      )
    case 'permission_request':
      return supersededWrap(
        <PermissionDialog
          requestId={message.requestId}
          toolName={message.toolName}
          input={message.input}
          description={message.description}
        />
      )
    case 'error': {
      const errorKey = message.code ? `error.${message.code}` as TranslationKey : null
      const errorText = errorKey ? t(errorKey) : null
      const displayMessage = (errorText && errorText !== errorKey) ? errorText : message.message
      const showRawDetail =
        Boolean(message.message) &&
        message.message.trim() !== '' &&
        message.message !== displayMessage
      return supersededWrap(
        <div className="mb-2 px-4 py-2 rounded-lg border border-[var(--color-error)]/20 bg-[var(--color-error-container)]/28 text-sm text-[var(--color-error)]">
          <strong>Error:</strong> {displayMessage}
          {showRawDetail && (
            <div className="mt-1 whitespace-pre-wrap text-xs text-[var(--color-on-error-container)]/85">
              {message.message}
            </div>
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
        />
      )
    case 'plan_mode_blocked':
      return (
        <PlanModeBlockedNotice
          tools={message.tools}
          superseded={message.superseded}
        />
      )
  }
}, areMessageBlockPropsEqual)
