// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useRef, useEffect, useCallback, useMemo } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useTranslation } from '../../i18n'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useUIStore } from '../../stores/uiStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useSettingsStore } from '../../stores/settingsStore'
import { useSessionRuntimeStore } from '../../stores/sessionRuntimeStore'
import { useTeamStore } from '../../stores/teamStore'
import { useProviderStore } from '../../stores/providerStore'
import { sessionsApi } from '../../api/sessions'
import { suggestionsApi, type PromptSuggestion } from '../../api/suggestions'
import { anyProviderHasModel } from '../../utils/modelAvailability'
import { isValidRuntimeSelection } from '../../utils/runtimeSelection'
import { enabledProviderModelIds } from '../../utils/providerModels'
import { surfaceToModelType } from '../../utils/modelTypes'
import type { ModelInfo } from '../../types/settings'
import { CodingModeSelector } from '../controls/CodingModeSelector'
import { ModelSelector } from '../controls/ModelSelector'
import { CODING_MODE_ACCENT } from '../../types/codingMode'
import type { AttachmentRef } from '../../types/chat'
import { AttachmentGallery } from './AttachmentGallery'
import { ProjectContextChip } from '../shared/ProjectContextChip'
import { DirectoryPicker } from '../shared/DirectoryPicker'
import { DesignerInlineControls } from '../designer/DesignerInlineControls'
import { useDesignerStore } from '../../stores/designerStore'
import { useDesignerCanvasStore, unitDisplayName } from '../../stores/designerCanvasStore'
import { DESIGN_UNIT_DND_MIME } from '../designer/DesignArtifactFrame'
import { FileSearchMenu, type FileSearchMenuHandle } from './FileSearchMenu'
import { LocalSlashCommandPanel, type LocalSlashCommandName } from './LocalSlashCommandPanel'
import { PrivacyBanner } from './PrivacyBanner'
import { ReviewCard } from './ReviewCard'
import { WorkersStrip } from './WorkersStrip'
import { TokenUsageRing } from './TokenUsageRing'
import { WorkspaceQueuePanel } from './WorkspaceQueuePanel'
import {
  FALLBACK_SLASH_COMMANDS,
  findSlashTrigger,
  mergeSlashCommands,
  replaceSlashToken,
  resolveSlashUiAction,
} from './composerUtils'
import { useCredentialsStore } from '../../stores/credentialsStore'
import { useBrowserPanelStore } from '../../stores/browserPanelStore'
import { dockListTabs, type BrowserDockTabInfo } from '../../lib/browserDock'
import { bindDebugTab, unbindDebugTab, bindPrototypeRef, bindPrototypeFigma, unbindPrototypeRef } from '../../lib/debugTabBind'

type GitInfo = { branch: string | null; repoName: string | null; workDir: string; changedFiles: number }

type Attachment = {
  id: string
  name: string
  type: 'image' | 'file'
  mimeType?: string
  previewUrl?: string
  data?: string
}

type ChatInputProps = {
  variant?: 'default' | 'hero'
}

const EMPTY_DOCK_TABS: BrowserDockTabInfo[] = []
const EMPTY_SLASH_COMMANDS: Array<{ name: string; description: string }> = []

export function ChatInput({ variant = 'default' }: ChatInputProps) {
  const t = useTranslation()
  const [input, setInput] = useState('')
  const [attachments, setAttachments] = useState<Attachment[]>([])
  const [designRef, setDesignRef] = useState<string | null>(null)
  const [designRefElement, setDesignRefElement] = useState<
    { id: string; label: string } | null
  >(null)
  const [slashMenuOpen, setSlashMenuOpen] = useState(false)
  const [fileSearchOpen, setFileSearchOpen] = useState(false)
  const [localSlashPanel, setLocalSlashPanel] = useState<LocalSlashCommandName | null>(null)
  const [atFilter, setAtFilter] = useState('')
  const [atCursorPos, setAtCursorPos] = useState(-1)
  const [slashFilter, setSlashFilter] = useState('')
  const [slashSelectedIndex, setSlashSelectedIndex] = useState(0)
  const composingRef = useRef(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const slashMenuRef = useRef<HTMLDivElement>(null)
  const fileSearchRef = useRef<FileSearchMenuHandle>(null)
  const slashItemRefs = useRef<(HTMLButtonElement | null)[]>([])
  const composerRootRef = useRef<HTMLDivElement>(null)
  const sendMessage = useChatStore((s) => s.sendMessage)
  const stopGeneration = useChatStore((s) => s.stopGeneration)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionView = useChatStore(
    useShallow((s) => {
      const st = activeTabId ? s.sessions[activeTabId] : undefined
      return {
        chatState: st?.chatState ?? 'idle',
        stopRequested: st?.stopRequested ?? false,
        slashCommands: st?.slashCommands,
        composerPrefill: st?.composerPrefill ?? null,
        pendingPermission: st?.pendingPermission ?? null,
      }
    }),
  )
  const chatState = sessionView.chatState
  const stopRequested = sessionView.stopRequested
  const slashCommands = sessionView.slashCommands ?? EMPTY_SLASH_COMMANDS
  const composerPrefill = sessionView.composerPrefill
  const globalCodingMode = useSettingsStore((s) => s.codingMode)
  const sessionCodingMode = useChatStore((s) =>
    activeTabId ? s.sessionCodingMode[activeTabId] : undefined,
  )
  const codingMode = sessionCodingMode ?? globalCodingMode
  const isPlanMode = codingMode === 'plan'
  const isDebugMode = codingMode === 'debug'
  const modeAccent = CODING_MODE_ACCENT[codingMode]
  const designerSelectedId = useDesignerStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.selectedSubmodeId ?? null : null,
  )
  const designerCatalog = useDesignerStore((s) => s.catalog)
  const designerSelectedModel = useDesignerStore((s) => {
    if (!activeTabId) return ''
    const session = s.sessions[activeTabId]
    const submodeId = session?.selectedSubmodeId
    return submodeId
      ? String(session?.paramsBySubmode[submodeId]?.model ?? '')
      : ''
  })
  const designerSetParam = useDesignerStore((s) => s.setParam)
  const credentialsList = useCredentialsStore((s) => s.credentials)
  const credentialsHasFetched = useCredentialsStore((s) => s.hasFetched)
  const credentialsIsLoading = useCredentialsStore((s) => s.isLoading)
  const fetchCredentials = useCredentialsStore((s) => s.fetchAll)
  const [credPanelOpen, setCredPanelOpen] = useState(false)
  const credPanelRef = useRef<HTMLDivElement>(null)
  const [tabBindOpen, setTabBindOpen] = useState(false)
  const tabBindRef = useRef<HTMLDivElement>(null)
  const [refreshedDockTabs, setRefreshedDockTabs] = useState<BrowserDockTabInfo[] | null>(null)
  const [protoBindOpen, setProtoBindOpen] = useState(false)
  const protoBindRef = useRef<HTMLDivElement>(null)
  const [refreshedProtoTabs, setRefreshedProtoTabs] = useState<BrowserDockTabInfo[] | null>(null)
  const boundTabId = useBrowserPanelStore((s) =>
    activeTabId ? s.panels[activeTabId]?.preferredTestTabId ?? null : null,
  )
  const protoTabId = useBrowserPanelStore((s) =>
    activeTabId ? s.panels[activeTabId]?.prototypeRefTabId ?? null : null,
  )
  const protoFigmaUrl = useBrowserPanelStore((s) =>
    activeTabId ? s.panels[activeTabId]?.prototypeRefFigmaUrl ?? null : null,
  )
  const [protoFigmaDraft, setProtoFigmaDraft] = useState('')
  const dockTabsFromPanel = useBrowserPanelStore((s) =>
    activeTabId ? s.panels[activeTabId]?.tabs ?? EMPTY_DOCK_TABS : EMPTY_DOCK_TABS,
  )
  const activeSession = useSessionStore((state) => activeTabId ? state.sessions.find((session) => session.id === activeTabId) ?? null : null)
  const memberInfo = useTeamStore((s) => activeTabId ? s.getMemberBySessionId(activeTabId) : null)
  const [gitInfo, setGitInfo] = useState<GitInfo | null>(null)
  const hasMessages = useChatStore((s) => activeTabId ? (s.sessions[activeTabId]?.messages?.length ?? 0) > 0 : false)
  const providers = useProviderStore((s) => s.providers)
  const settingsCurrentModel = useSettingsStore((s) => s.currentModel)
  const settingsAvailableModels = useSettingsStore((s) => s.availableModels)
  const sessionRuntimeSelection = useSessionRuntimeStore((s) => activeTabId ? s.selections[activeTabId] : undefined)
  const openSettingsOverlay = useUIStore((s) => s.openSettingsOverlay)

  const designerSurface = useMemo(
    () =>
      codingMode === 'designer'
        ? designerCatalog.find((s) => s.id === designerSelectedId)?.surface ?? null
        : null,
    [codingMode, designerCatalog, designerSelectedId],
  )
  const designerMediaType = surfaceToModelType(designerSurface)
  const designerMediaModel =
    designerMediaType && designerSelectedId ? designerSelectedModel : ''
  const designerMediaPool = useMemo<ModelInfo[]>(() => {
    if (!designerMediaType) return []
    const seen = new Set<string>()
    const out: ModelInfo[] = []
    for (const provider of providers) {
      for (const id of enabledProviderModelIds(provider)) {
        if (seen.has(id)) continue
        seen.add(id)
        out.push({ id, name: id, description: '', context: '' })
      }
    }
    return out
  }, [providers, designerMediaType])

  const isMemberSession = !!memberInfo
  const isActive = chatState !== 'idle'
  const hasModel = useMemo(() => {
    if (isMemberSession) return true
    if (
      sessionRuntimeSelection &&
      isValidRuntimeSelection(sessionRuntimeSelection, providers)
    ) {
      return true
    }
    if (anyProviderHasModel(providers)) return true
    if ((settingsAvailableModels?.length ?? 0) > 0) return true
    if (settingsCurrentModel) return true
    return false
  }, [
    isMemberSession,
    sessionRuntimeSelection,
    providers,
    settingsAvailableModels,
    settingsCurrentModel,
  ])
  const [stopCooldown, setStopCooldown] = useState(false)
  const [sendButtonHover, setSendButtonHover] = useState(false)
  const stopCooldownTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => {
    if (!isActive && !stopRequested) {
      setStopCooldown(false)
      if (stopCooldownTimerRef.current) {
        clearTimeout(stopCooldownTimerRef.current)
        stopCooldownTimerRef.current = null
      }
    }
  }, [isActive, stopRequested])

  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent).detail as
        | { sessionId?: string; relPath?: string; element?: string; elementLabel?: string }
        | undefined
      if (!detail?.relPath) return
      if (detail.sessionId && detail.sessionId !== activeTabId) return
      setDesignRef(detail.relPath)
      setDesignRefElement(
        detail.element
          ? { id: detail.element, label: detail.elementLabel ?? detail.element }
          : null,
      )
      requestAnimationFrame(() => textareaRef.current?.focus())
    }
    window.addEventListener('designer:composer-ref', handler)
    return () => window.removeEventListener('designer:composer-ref', handler)
  }, [activeTabId])

  useEffect(() => {
    if (!isDebugMode) {
      setCredPanelOpen(false)
      setTabBindOpen(false)
      setProtoBindOpen(false)
      return
    }
    if (!credentialsHasFetched && !credentialsIsLoading) {
      void fetchCredentials()
    }
  }, [isDebugMode, credentialsHasFetched, credentialsIsLoading, fetchCredentials])

  useEffect(() => {
    if (!credPanelOpen) return
    const handleMouseDown = (event: MouseEvent) => {
      const target = event.target as Node | null
      const panel = credPanelRef.current
      if (!panel || !target) return
      if (panel.contains(target)) return
      setCredPanelOpen(false)
    }
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setCredPanelOpen(false)
    }
    document.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('keydown', handleKey)
    return () => {
      document.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('keydown', handleKey)
    }
  }, [credPanelOpen])

  useEffect(() => {
    if (!tabBindOpen) return
    const handleMouseDown = (event: MouseEvent) => {
      const target = event.target as Node | null
      const panel = tabBindRef.current
      if (!panel || !target) return
      if (panel.contains(target)) return
      setTabBindOpen(false)
    }
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setTabBindOpen(false)
    }
    document.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('keydown', handleKey)
    return () => {
      document.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('keydown', handleKey)
    }
  }, [tabBindOpen])

  const refreshDockTabsForBinder = useCallback(async () => {
    if (!activeTabId) return
    try {
      const sessionTabs = await dockListTabs(activeTabId)
      setRefreshedDockTabs(sessionTabs)
      if (boundTabId != null) {
        if (!sessionTabs.some((tab) => tab.id === boundTabId)) {
          unbindDebugTab(activeTabId, boundTabId)
        }
      }
    } catch (err) {
      console.warn('[ChatInput] refresh dock tabs failed', err)
    }
  }, [activeTabId, boundTabId])

  useEffect(() => {
    if (!tabBindOpen) return
    void refreshDockTabsForBinder()
  }, [tabBindOpen, refreshDockTabsForBinder])

  const handleBindTab = useCallback(
    (tabId: number) => {
      if (!activeTabId) return
      if (boundTabId === tabId) {
        setTabBindOpen(false)
        return
      }
      bindDebugTab(activeTabId, tabId)
      setTabBindOpen(false)
    },
    [activeTabId, boundTabId],
  )

  const handleUnbindTab = useCallback(() => {
    if (!activeTabId || boundTabId == null) return
    unbindDebugTab(activeTabId, boundTabId)
    setTabBindOpen(false)
  }, [activeTabId, boundTabId])

  const refreshProtoTabsForBinder = useCallback(async () => {
    if (!activeTabId) return
    try {
      const sessionTabs = await dockListTabs(activeTabId)
      setRefreshedProtoTabs(sessionTabs)
      if (protoTabId != null) {
        if (!sessionTabs.some((tab) => tab.id === protoTabId)) {
          unbindPrototypeRef(activeTabId)
        }
      }
    } catch (err) {
      console.warn('[ChatInput] refresh proto tabs failed', err)
    }
  }, [activeTabId, protoTabId])

  useEffect(() => {
    if (!protoBindOpen) return
    void refreshProtoTabsForBinder()
  }, [protoBindOpen, refreshProtoTabsForBinder])

  useEffect(() => {
    if (!protoBindOpen) return
    const handleMouseDown = (event: MouseEvent) => {
      const target = event.target as Node | null
      const panel = protoBindRef.current
      if (!panel || !target) return
      if (panel.contains(target)) return
      setProtoBindOpen(false)
    }
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setProtoBindOpen(false)
    }
    document.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('keydown', handleKey)
    return () => {
      document.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('keydown', handleKey)
    }
  }, [protoBindOpen])

  const handleBindProto = useCallback(
    (tabId: number) => {
      if (!activeTabId) return
      if (protoTabId === tabId) {
        setProtoBindOpen(false)
        return
      }
      bindPrototypeRef(activeTabId, tabId)
      setProtoBindOpen(false)
    },
    [activeTabId, protoTabId],
  )

  const handleUnbindProto = useCallback(() => {
    if (!activeTabId) return
    unbindPrototypeRef(activeTabId)
    setProtoBindOpen(false)
  }, [activeTabId])

  const handleBindProtoFigma = useCallback(() => {
    if (!activeTabId) return
    const url = protoFigmaDraft.trim()
    if (!url) return
    if (!url.includes('figma.com/')) {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('designer.figma.invalidUrl'),
      })
      return
    }
    bindPrototypeFigma(activeTabId, url)
    setProtoFigmaDraft('')
    setProtoBindOpen(false)
  }, [activeTabId, protoFigmaDraft, t])

  const insertCredentialPlaceholder = useCallback((name: string) => {
    const placeholder = `\${cred.${name}}`
    const ta = textareaRef.current
    if (!ta) {
      setInput((prev) => `${prev}${placeholder}`)
      return
    }
    const start = ta.selectionStart ?? input.length
    const end = ta.selectionEnd ?? input.length
    const next = `${input.slice(0, start)}${placeholder}${input.slice(end)}`
    setInput(next)
    requestAnimationFrame(() => {
      const pos = start + placeholder.length
      ta.focus()
      ta.setSelectionRange(pos, pos)
    })
  }, [input])
  useEffect(() => {
    return () => {
      if (stopCooldownTimerRef.current) {
        clearTimeout(stopCooldownTimerRef.current)
        stopCooldownTimerRef.current = null
      }
    }
  }, [])
  const handleStopClick = () => {
    if (!activeTabId) return
    if (stopCooldown || stopRequested) return
    setStopCooldown(true)
    if (stopCooldownTimerRef.current) {
      clearTimeout(stopCooldownTimerRef.current)
    }
    stopCooldownTimerRef.current = setTimeout(() => {
      setStopCooldown(false)
      stopCooldownTimerRef.current = null
    }, 800)
    stopGeneration(activeTabId)
  }
  const showStopping = isActive && (stopRequested || stopCooldown)
  const isWorkspaceMissing = activeSession?.workDirExists === false
  const canSubmit =
    !isWorkspaceMissing &&
    (isMemberSession || hasModel) &&
    (input.trim().length > 0 || (!isMemberSession && attachments.length > 0))
  const actAsStopButton = !isMemberSession && isActive && !canSubmit
  const isHeroComposer = variant === 'hero' && !isMemberSession
  const resolvedWorkDir = activeSession?.workDir || gitInfo?.workDir || undefined
  const showNoModelBanner = !isMemberSession && !hasModel

  const [promptSuggestions, setPromptSuggestions] = useState<PromptSuggestion[]>([])
  useEffect(() => {
    if (!isHeroComposer || isWorkspaceMissing) {
      setPromptSuggestions([])
      return
    }
    let cancelled = false
    suggestionsApi
      .list()
      .then((res) => {
        if (!cancelled) setPromptSuggestions(res.suggestions.slice(0, 4))
      })
      .catch(() => {
        if (!cancelled) setPromptSuggestions([])
      })
    return () => {
      cancelled = true
    }
  }, [isHeroComposer, isWorkspaceMissing, resolvedWorkDir])

  useEffect(() => {
    textareaRef.current?.focus()
  }, [isActive])

  const prevDraftSessionRef = useRef<string | null>(null)
  const setComposerDraft = useChatStore((s) => s.setComposerDraft)
  useEffect(() => {
    const prev = prevDraftSessionRef.current
    const next = activeTabId ?? null
    if (prev === next) {
      prevDraftSessionRef.current = next
      return
    }
    if (prev) {
      setComposerDraft(prev, {
        text: input,
        attachments: attachments.map((att) => ({
          type: att.type,
          name: att.name,
          data: att.data,
          mimeType: att.mimeType,
        })),
        slashMenuOpen,
      })
    }
    if (next) {
      const draft = useChatStore.getState().sessions[next]?.composerDraft
      if (draft) {
        setInput(draft.text ?? '')
        setAttachments(
          (draft.attachments ?? [])
            .filter((a) => a.type === 'image' || a.data)
            .map((a, index) => ({
              id: `draft-${next}-${index}-${Date.now()}`,
              name: a.name,
              type: a.type,
              mimeType: a.mimeType,
              previewUrl: a.type === 'image' ? a.data : undefined,
              data: a.data,
            })),
        )
        setSlashMenuOpen(!!draft.slashMenuOpen)
      } else {
        setInput('')
        setAttachments([])
        setSlashMenuOpen(false)
      }
    } else {
      setInput('')
      setAttachments([])
      setSlashMenuOpen(false)
    }
    setFileSearchOpen(false)
    setLocalSlashPanel(null)
    setSlashFilter('')
    setAtFilter('')
    setAtCursorPos(-1)
    setSlashSelectedIndex(0)
    prevDraftSessionRef.current = next
  }, [activeTabId, setComposerDraft])

  useEffect(() => {
    const el = composerRootRef.current
    if (!el) return
    const root = document.documentElement
    const update = () => {
      const h = Math.round(el.getBoundingClientRect().height)
      root.style.setProperty('--composer-height', `${h}px`)
    }
    update()
    const ro = new ResizeObserver(update)
    ro.observe(el)
    return () => {
      ro.disconnect()
      root.style.setProperty('--composer-height', '0px')
    }
  }, [])

  useEffect(() => {
    if (!composerPrefill) return

    setInput(composerPrefill.text)
    setAttachments(
      (composerPrefill.attachments ?? [])
        .filter((attachment) => attachment.type === 'image' || attachment.data)
        .map((attachment, index) => ({
          id: `rewind-prefill-${composerPrefill.nonce}-${index}`,
          name: attachment.name,
          type: attachment.type,
          mimeType: attachment.mimeType,
          previewUrl: attachment.type === 'image' ? attachment.data : undefined,
          data: attachment.data,
        })),
    )
    setSlashMenuOpen(false)
    setFileSearchOpen(false)
    setSlashFilter('')
    setAtFilter('')
    setAtCursorPos(-1)

    requestAnimationFrame(() => {
      const el = textareaRef.current
      el?.focus()
      const cursor = composerPrefill.text.length
      el?.setSelectionRange(cursor, cursor)
    })
  }, [composerPrefill])

  useEffect(() => {
    if (!activeTabId) {
      setGitInfo(null)
      return
    }
    if (isMemberSession) {
      setGitInfo(null)
      return
    }
    sessionsApi.getGitInfo(activeTabId).then(setGitInfo).catch(() => setGitInfo(null))
  }, [activeTabId, isMemberSession])

  useEffect(() => {
    if (!isMemberSession) return
    setAttachments([])
    setSlashMenuOpen(false)
    setFileSearchOpen(false)
  }, [isMemberSession, activeTabId])

  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }, [input])

  useEffect(() => {
    if (!slashMenuOpen) return
    const handleClick = (event: MouseEvent) => {
      if (
        slashMenuRef.current &&
        !slashMenuRef.current.contains(event.target as Node) &&
        textareaRef.current &&
        !textareaRef.current.contains(event.target as Node)
      ) {
        setSlashMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [slashMenuOpen])

  useEffect(() => {
    if (!localSlashPanel) return
    const handleClick = (event: MouseEvent) => {
      if (
        slashMenuRef.current &&
        !slashMenuRef.current.contains(event.target as Node) &&
        textareaRef.current &&
        !textareaRef.current.contains(event.target as Node)
      ) {
        setLocalSlashPanel(null)
      }
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [localSlashPanel])

  useEffect(() => {
    if (!fileSearchOpen) return
    const handleClick = (event: MouseEvent) => {
      const menu = document.getElementById('file-search-menu')
      if (
        menu &&
        !menu.contains(event.target as Node) &&
        textareaRef.current &&
        !textareaRef.current.contains(event.target as Node)
      ) {
        setFileSearchOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [fileSearchOpen])

  const filteredCommands = useMemo(() => {
    const source = mergeSlashCommands(slashCommands, FALLBACK_SLASH_COMMANDS)
    if (!slashFilter) return source
    const lower = slashFilter.toLowerCase()
    return source.filter((command) => (
      command.name.toLowerCase().includes(lower) ||
      command.description.toLowerCase().includes(lower)
    ))
  }, [slashCommands, slashFilter])

  const exactSlashCommand = useMemo(() => {
    const normalized = slashFilter.trim().toLowerCase()
    if (!normalized) return null
    return filteredCommands.find((command) => command.name.toLowerCase() === normalized) ?? null
  }, [filteredCommands, slashFilter])

  useEffect(() => {
    setSlashSelectedIndex(0)
  }, [slashFilter])

  useEffect(() => {
    const activeItem = slashMenuOpen ? slashItemRefs.current[slashSelectedIndex] : null
    if (activeItem && typeof activeItem.scrollIntoView === 'function') {
      activeItem.scrollIntoView({ block: 'nearest' })
    }
  }, [slashMenuOpen, slashSelectedIndex])

  const detectSlashTrigger = useCallback((value: string, cursorPos: number) => {
    const token = findSlashTrigger(value, cursorPos)
    if (!token) {
      setSlashMenuOpen(false)
      return
    }

    setFileSearchOpen(false)
    setSlashFilter(token.filter)
    setSlashMenuOpen(true)
  }, [])

  const detectAtTrigger = useCallback((value: string, cursorPos: number) => {
    const textBeforeCursor = value.slice(0, cursorPos)
    let pos = -1

    for (let i = textBeforeCursor.length - 1; i >= 0; i--) {
      const ch = textBeforeCursor[i]!
      if (ch === '@') {
        if (i === 0 || /\s/.test(textBeforeCursor[i - 1]!)) {
          pos = i
          break
        }
        break
      }
      if (/\s/.test(ch)) {
        break
      }
    }

    if (pos < 0) {
      setFileSearchOpen(false)
      setAtFilter('')
      setAtCursorPos(-1)
      return
    }

    const filter = textBeforeCursor.slice(pos + 1)
    setAtFilter(filter)
    setAtCursorPos(cursorPos)
    setSlashMenuOpen(false)
    setFileSearchOpen(true)
  }, [])

  const handleInputChange = (event: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = event.target.value
    if (isMemberSession) {
      setInput(value)
      return
    }
    const cursorPos = event.target.selectionStart ?? value.length
    setInput(value)
    detectSlashTrigger(value, cursorPos)
    detectAtTrigger(value, cursorPos)
  }

  const selectSlashCommand = useCallback((command: string) => {
    const el = textareaRef.current
    if (!el) return
    const cursorPos = el.selectionStart ?? input.length
    const replacement = replaceSlashToken(input, cursorPos, command)
    setInput(replacement.value)
    setSlashMenuOpen(false)
    requestAnimationFrame(() => {
      el.focus()
      el.setSelectionRange(replacement.cursorPos, replacement.cursorPos)
    })
  }, [input])

  const handleSubmit = () => {
    if (isPendingQuestion) {

      window.dispatchEvent(new CustomEvent('plan:question:submit'))
      return
    }
    const text = input.trim()
    if ((!text && (!attachments.length || isMemberSession)) || isWorkspaceMissing) return

    if (codingMode === 'designer' && !isMemberSession && activeTabId) {
      const ds = useDesignerStore.getState()
      const dsSession = ds.sessions[activeTabId]
      const submodeId = dsSession?.selectedSubmodeId ?? null
      if (submodeId && text) {
        const submodeMeta = ds.catalog.find((s) => s.id === submodeId)
        const submodeParams = dsSession?.paramsBySubmode[submodeId] ?? {}
        const isZh = useSettingsStore.getState().locale === 'zh'
        for (const field of submodeMeta?.fields ?? []) {
          if (!field.required) continue
          const raw = submodeParams[field.key]
          const value = typeof raw === 'string' ? raw.trim() : ''
          if (value) {
            if (field.key === 'figmaUrl' && !value.includes('figma.com/')) {
              useUIStore.getState().addToast({
                type: 'error',
                message: t('designer.figma.invalidUrl'),
              })
              return
            }
            continue
          }
          if (field.key === 'figmaUrl' && text.includes('figma.com/')) continue
          useUIStore.getState().addToast({
            type: 'error',
            message: t('designer.inline.requiredMissing').replace(
              '{label}',
              isZh ? field.labelZh : field.labelEn,
            ),
          })
          return
        }
        const designGeneration = {
          submode: submodeId,
          params: dsSession?.paramsBySubmode[submodeId] ?? {},
          refArtifact: designRef ?? undefined,
          refArtifactName: designRef ? designRefName : undefined,
          refElement: designRefElement?.id,
          refElementLabel: designRefElement?.label,
        }
        setInput('')
        setAttachments([])
        setDesignRef(null)
        setDesignRefElement(null)
        setSlashMenuOpen(false)
        setFileSearchOpen(false)
        setLocalSlashPanel(null)
        useChatStore.getState().clearComposerDraft(activeTabId)
        sendMessage(activeTabId, text, undefined, { designGeneration })
        return
      }
    }

    const slashUiAction = !isMemberSession && text.startsWith('/') ? resolveSlashUiAction(text.slice(1)) : null
    if (slashUiAction?.type === 'panel') {
      setLocalSlashPanel(slashUiAction.command as LocalSlashCommandName)
      setInput('')
      setSlashMenuOpen(false)
      setFileSearchOpen(false)
      return
    }

    if (slashUiAction?.type === 'settings') {
      useUIStore.getState().openSettingsOverlay(slashUiAction.tab)
      setInput('')
      setSlashMenuOpen(false)
      setFileSearchOpen(false)
      return
    }

    const attachmentPayload: AttachmentRef[] = attachments.map((attachment) => ({
      type: attachment.type,
      name: attachment.name,
      data: attachment.data,
      mimeType: attachment.mimeType,
    }))

    const targetTabId = activeTabId!
    setInput('')
    setAttachments([])
    setSlashMenuOpen(false)
    setFileSearchOpen(false)
    setLocalSlashPanel(null)
    if (activeTabId) {
      useChatStore.getState().clearComposerDraft(activeTabId)
    }
    sendMessage(targetTabId, text, attachmentPayload)
  }

  const handleKeyDown = (event: React.KeyboardEvent) => {

    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return

    if (fileSearchOpen) {
      const key = event.key
      if (key === 'ArrowDown' || key === 'ArrowUp' || key === 'Enter' || key === 'Tab' || key === 'Escape') {
        event.preventDefault()
        if (key === 'Escape') {
          setFileSearchOpen(false)
          setAtFilter('')
          setAtCursorPos(-1)
          return
        }
        fileSearchRef.current?.handleKeyDown(event.nativeEvent)
        return
      }

      return
    }

    if (slashMenuOpen && filteredCommands.length > 0) {
      if (event.key === 'ArrowDown') {
        event.preventDefault()
        setSlashSelectedIndex((prev) => (prev + 1) % filteredCommands.length)
        return
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault()
        setSlashSelectedIndex((prev) => (prev - 1 + filteredCommands.length) % filteredCommands.length)
        return
      }
      if (event.key === 'Enter') {
        if (exactSlashCommand && slashFilter.trim().toLowerCase() === exactSlashCommand.name.toLowerCase()) {
          event.preventDefault()
          handleSubmit()
          return
        }
        event.preventDefault()
        const selected = filteredCommands[slashSelectedIndex]
        if (selected) selectSlashCommand(selected.name)
        return
      }
      if (event.key === 'Tab') {
        event.preventDefault()
        const selected = filteredCommands[slashSelectedIndex]
        if (selected) selectSlashCommand(selected.name)
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        setSlashMenuOpen(false)
        return
      }
    }

    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      handleSubmit()
    }
  }

  const handlePaste = (event: React.ClipboardEvent) => {
    if (isMemberSession) return
    const items = event.clipboardData?.items
    if (!items) return

    let hasImage = false
    for (let i = 0; i < items.length; i += 1) {
      const item = items[i]
      if (!item || !item.type.startsWith('image/')) continue

      hasImage = true
      event.preventDefault()
      const file = item.getAsFile()
      if (!file) continue

      const id = `att-${Date.now()}-${Math.random().toString(36).slice(2)}`
      const reader = new FileReader()
      reader.onload = () => {
        setAttachments((prev) => [
          ...prev,
          {
            id,
            name: `pasted-image-${Date.now()}.png`,
            type: 'image',
            mimeType: file.type || 'image/png',
            previewUrl: reader.result as string,
            data: reader.result as string,
          },
        ])
      }
      reader.readAsDataURL(file)
    }

    if (!hasImage) return
  }

  const handleFileSelect = (event: React.ChangeEvent<HTMLInputElement>) => {
    if (isMemberSession) return
    const files = event.target.files
    if (!files) return

    Array.from(files).forEach((file) => {
      const id = `att-${Date.now()}-${Math.random().toString(36).slice(2)}`
      const isImage = file.type.startsWith('image/')
      const reader = new FileReader()
      reader.onload = () => {
        setAttachments((prev) => [
          ...prev,
          {
            id,
            name: file.name,
            type: isImage ? 'image' : 'file',
            mimeType: file.type || undefined,
            previewUrl: isImage ? (reader.result as string) : undefined,
            data: reader.result as string,
          },
        ])
      }
      reader.readAsDataURL(file)
    })

    event.target.value = ''
  }

  const handleDrop = (event: React.DragEvent) => {
    event.preventDefault()
    if (isMemberSession) return
    const designUnitPayload = event.dataTransfer.getData(DESIGN_UNIT_DND_MIME)
    if (designUnitPayload) {
      try {
        const parsed = JSON.parse(designUnitPayload) as {
          relPath?: string
          sessionId?: string
        }
        if (parsed.relPath && (!parsed.sessionId || parsed.sessionId === activeTabId)) {
          setDesignRef(parsed.relPath)
          setDesignRefElement(null)
          return
        }
      } catch {
        /* ignore malformed payload */
      }
    }
    const files = event.dataTransfer.files
    if (files.length > 0) {
      const fakeEvent = { target: { files } } as React.ChangeEvent<HTMLInputElement>
      handleFileSelect(fakeEvent)
    }
  }

  const removeAttachment = (id: string) => {
    setAttachments((prev) => prev.filter((attachment) => attachment.id !== id))
  }

  const isPendingQuestion =
    !!sessionView.pendingPermission &&
    (sessionView.pendingPermission.toolName === 'ask_question' ||
      sessionView.pendingPermission.toolName === 'AskUserQuestion')

  const designRefUnit = useDesignerCanvasStore((s) => {
    if (!designRef || !activeTabId) return null
    return s.panels[activeTabId]?.units.find((u) => u.relPath === designRef) ?? null
  })
  const designRefName = designRefUnit
    ? unitDisplayName(designRefUnit)
    : (designRef?.split('/').pop() ?? '')
  const designRefIcon =
    designRefUnit?.surface === 'image'
      ? 'image'
      : designRefUnit?.surface === 'video'
        ? 'movie'
        : designRefUnit?.surface === 'audio'
          ? 'graphic_eq'
          : designRefUnit?.surface === 'deck'
            ? 'co_present'
            : 'draw'

  const composerPlaceholder = isPendingQuestion
    ? t('composer.askDetailsPlaceholder')
    : isDebugMode && !isWorkspaceMissing && !isMemberSession
      ? t('debug.qa.inputPlaceholder')
      : isHeroComposer
        ? t('empty.placeholder')
        : isWorkspaceMissing
          ? t('chat.placeholderMissing')
          : isMemberSession
            ? t('teams.memberPlaceholder')
            : t('chat.placeholder')

  return (
    <div
      ref={composerRootRef}
      className={isHeroComposer ? 'bg-[var(--color-surface)] px-8 pb-4' : 'bg-[var(--color-surface)] px-4 py-3'}
    >
      <div className={isHeroComposer ? 'mx-auto flex w-full max-w-3xl flex-col gap-1.5' : 'mx-auto max-w-[860px]'}>
        {!isMemberSession && isDebugMode && (
          <PrivacyBanner sessionId={activeTabId ?? null} />
        )}
        {showNoModelBanner && (
          <div
            role="status"
            className="mb-1.5 flex items-center gap-2 rounded-[var(--radius-md)] border border-[var(--color-warning)]/35 bg-[var(--color-warning-container)]/25 px-3 py-2 text-[12px] text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined flex-shrink-0 text-[16px] text-[var(--color-warning)]">
              info
            </span>
            <span className="flex-1 leading-snug">{t('chat.noModel.banner')}</span>
            <button
              type="button"
              onClick={() => openSettingsOverlay('providers')}
              className="flex-shrink-0 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
            >
              {t('chat.noModel.openSettings')}
            </button>
          </div>
        )}
        {!isMemberSession && <WorkersStrip sessionId={activeTabId} />}
        {!isMemberSession && <ReviewCard />}
        {!isMemberSession && <WorkspaceQueuePanel sessionId={activeTabId} />}
        <div
          className={isHeroComposer
            ? 'glass-panel relative flex min-h-[100px] flex-col gap-1.5 rounded-xl px-3 py-2.5 transition-colors'
            : 'glass-panel relative min-h-[100px] rounded-xl px-3 py-2.5 transition-colors'}
          onDragOver={(event) => event.preventDefault()}
          onDrop={handleDrop}
        >
          {!isMemberSession && fileSearchOpen && (
            <FileSearchMenu
              ref={fileSearchRef}
              cwd={resolvedWorkDir || ''}
              filter={atFilter}
              onSelect={(_path, name) => {
                if (atCursorPos >= 0) {

                  const newValue = `${input.slice(0, atCursorPos)}${name}${input.slice(atCursorPos)}`
                  const newCursorPos = atCursorPos + name.length
                  setInput(newValue)
                  setFileSearchOpen(false)
                  setAtFilter('')
                  setAtCursorPos(-1)
                  void textareaRef.current?.focus()
                  requestAnimationFrame(() => {
                    textareaRef.current?.setSelectionRange(newCursorPos, newCursorPos)
                  })
                }
              }}
            />
          )}

          {!isMemberSession && localSlashPanel && (
            <div ref={slashMenuRef}>
              <LocalSlashCommandPanel
                command={localSlashPanel}
                cwd={resolvedWorkDir}
                onClose={() => setLocalSlashPanel(null)}
              />
            </div>
          )}

          {!isMemberSession && slashMenuOpen && filteredCommands.length > 0 && (
            <div
              ref={slashMenuRef}
              className="absolute bottom-full left-0 right-0 z-50 mb-2 overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]"
            >
              <div className="max-h-[300px] overflow-y-auto py-1">
                {filteredCommands.map((command, index) => (
                  <button
                    key={command.name}
                    ref={(el) => { slashItemRefs.current[index] = el }}
                    onClick={() => selectSlashCommand(command.name)}
                    onMouseEnter={() => setSlashSelectedIndex(index)}
                    className={`flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                      index === slashSelectedIndex
                        ? 'bg-[var(--color-surface-hover)]'
                        : 'hover:bg-[var(--color-surface-hover)]'
                    }`}
                  >
                    <span className="shrink-0 text-sm font-semibold text-[var(--color-text-primary)]">
                      /{command.name}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-xs text-[var(--color-text-tertiary)]">
                      {command.description}
                    </span>
                  </button>
                ))}
              </div>
              <div className="flex items-center gap-1.5 border-t border-[var(--color-border)] px-4 py-2 text-xs text-[var(--color-text-tertiary)]">
                <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-1.5 py-0.5 font-mono text-[10px]">Up/Down</kbd>
                <span>{t('chat.navigate')}</span>
                <kbd className="ml-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-1.5 py-0.5 font-mono text-[10px]">Enter</kbd>
                <span>{t('chat.select')}</span>
                <kbd className="ml-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-1.5 py-0.5 font-mono text-[10px]">Esc</kbd>
                <span>{t('chat.dismiss')}</span>
              </div>
            </div>
          )}

          {attachments.length > 0 && (
            isHeroComposer ? (
              <AttachmentGallery attachments={attachments} variant="composer" onRemove={removeAttachment} />
            ) : (
              <div className="px-3 pt-3">
                <AttachmentGallery attachments={attachments} variant="composer" onRemove={removeAttachment} />
              </div>
            )
          )}

          {designRef && !isMemberSession && (
            <div className={isHeroComposer ? 'mb-1.5' : 'px-3 pt-2'}>
              <span
                className="inline-flex max-w-full items-center gap-1.5 rounded-lg border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 py-1 pl-1.5 pr-1 text-[11px] text-[var(--color-text-secondary)]"
                title={designRef}
              >
                <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded bg-[var(--color-accent)]/15">
                  <span className="material-symbols-outlined text-[13px] text-[var(--color-accent)]">
                    {designRefIcon}
                  </span>
                </span>
                <span className="flex min-w-0 flex-col leading-tight">
                  <span className="text-[9px] uppercase tracking-wide text-[var(--color-accent)]">
                    {designRefElement
                      ? `${t('designer.canvas.elementRef')}: ${designRefElement.label}`
                      : t('designer.canvas.editRefChip')}
                  </span>
                  <button
                    type="button"
                    onClick={() => {
                      if (activeTabId && designRefUnit) {
                        useDesignerCanvasStore.getState().setVisible(activeTabId, true)
                        useDesignerCanvasStore
                          .getState()
                          .focusUnit(activeTabId, designRefUnit.id)
                      }
                    }}
                    className="max-w-[200px] truncate text-left text-[11px] font-medium text-[var(--color-text-primary)] hover:underline"
                  >
                    {designRefName}
                  </button>
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setDesignRef(null)
                    setDesignRefElement(null)
                  }}
                  className="flex h-4 w-4 flex-shrink-0 items-center justify-center rounded hover:bg-[var(--color-surface-hover)]"
                  aria-label={t('designer.canvas.editRefRemove')}
                >
                  <span className="material-symbols-outlined text-[13px]">close</span>
                </button>
              </span>
            </div>
          )}

          {!isMemberSession && isDebugMode && (
            <div className="mb-1.5 flex flex-wrap items-start gap-2">
              <div className="relative" ref={credPanelRef}>
                <button
                  type="button"
                  onClick={() => {
                    setTabBindOpen(false)
                    setCredPanelOpen((v) => !v)
                  }}
                  className="inline-flex items-center gap-1 px-2 py-1 text-[11px] rounded border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                  title={t('debug.qa.insertCred')}
                >
                  <span className="material-symbols-outlined text-[14px]">key</span>
                  {t('debug.qa.panelTitle')}
                  <span className="material-symbols-outlined text-[14px]">
                    {credPanelOpen ? 'expand_less' : 'expand_more'}
                  </span>
                </button>
                {credPanelOpen && (
                  <div className="absolute z-20 mt-1.5 min-w-[280px] max-w-[420px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] p-2 shadow-md">
                    <div className="text-[11px] text-[var(--color-text-tertiary)] mb-1.5">
                      {t('credentials.placeholder.hint')}
                    </div>
                    {credentialsList.length === 0 ? (
                      <div className="text-[11px] italic text-[var(--color-text-tertiary)] py-1">
                        {t('credentials.empty')}
                      </div>
                    ) : (
                      <div className="flex flex-wrap gap-1.5">
                        {credentialsList.map((cred) => (
                          <button
                            key={cred.name}
                            type="button"
                            onClick={() => insertCredentialPlaceholder(cred.name)}
                            className="inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-mono rounded border border-[var(--color-border)] hover:border-[var(--color-border-focus)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-primary)]"
                            title={t('credentials.placeholder.insert').replace('{name}', cred.name)}
                          >
                            <span className="material-symbols-outlined text-[12px] text-[var(--color-text-secondary)]">
                              key
                            </span>
                            {cred.name}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>

              <div className="relative" ref={tabBindRef}>
                <button
                  type="button"
                  onClick={() => {
                    setCredPanelOpen(false)
                    setProtoBindOpen(false)
                    setTabBindOpen((v) => !v)
                  }}
                  className={
                    'inline-flex items-center gap-1 px-2 py-1 text-[11px] rounded border ' +
                    (boundTabId != null
                      ? 'border-[var(--color-brand)]/50 bg-[var(--color-brand)]/10 text-[var(--color-brand)]'
                      : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]')
                  }
                  title={t('debug.qa.bindTab.button')}
                >
                  <span className="material-symbols-outlined text-[14px]">tab</span>
                  {t('debug.qa.bindTab.button')}
                  {boundTabId != null && (
                    <span className="ml-0.5 inline-flex items-center gap-0.5 rounded-full bg-[var(--color-brand)]/15 px-1.5 py-px text-[10px] font-mono">
                      #{boundTabId}
                    </span>
                  )}
                  <span className="material-symbols-outlined text-[14px]">
                    {tabBindOpen ? 'expand_less' : 'expand_more'}
                  </span>
                </button>
                {tabBindOpen && (
                  <div className="absolute z-20 mt-1.5 min-w-[300px] max-w-[460px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] p-2 shadow-md">
                    <div className="mb-1.5 flex items-center justify-between gap-2">
                      <div className="text-[11px] text-[var(--color-text-tertiary)]">
                        {t('debug.qa.bindTab.hint')}
                      </div>
                      <button
                        type="button"
                        onClick={() => void refreshDockTabsForBinder()}
                        className="inline-flex items-center justify-center rounded border border-[var(--color-border)] p-0.5 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
                        title={t('debug.qa.bindTab.refresh')}
                      >
                        <span className="material-symbols-outlined text-[12px]">refresh</span>
                      </button>
                    </div>
                    {(() => {
                      const tabs = refreshedDockTabs ?? dockTabsFromPanel
                      if (tabs.length === 0) {
                        return (
                          <div className="text-[11px] italic text-[var(--color-text-tertiary)] py-1">
                            {t('debug.qa.bindTab.empty')}
                          </div>
                        )
                      }
                      return (
                        <div className="flex flex-col gap-1 max-h-[220px] overflow-auto">
                          {tabs.map((tab) => {
                            const isBound = tab.id === boundTabId
                            const ownerLabel = tab.owner === 'agent' ? t('debug.qa.bindTab.ownerAgent') : t('debug.qa.bindTab.ownerUser')
                            const label = tab.title?.trim() || tab.url?.trim() || `(tab ${tab.id})`
                            return (
                              <button
                                key={tab.id}
                                type="button"
                                onClick={() => handleBindTab(tab.id)}
                                className={
                                  'flex w-full items-center gap-2 rounded px-2 py-1 text-left text-[11px] transition-colors ' +
                                  (isBound
                                    ? 'border border-[var(--color-brand)]/40 bg-[var(--color-brand)]/10 text-[var(--color-brand)]'
                                    : 'border border-transparent hover:border-[var(--color-border)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-primary)]')
                                }
                              >
                                <span className="font-mono text-[10px] text-[var(--color-text-tertiary)]">
                                  #{tab.id}
                                </span>
                                <span className="rounded-full border border-[var(--color-border)] px-1.5 py-px text-[9px] uppercase tracking-wide text-[var(--color-text-tertiary)]">
                                  {ownerLabel}
                                </span>
                                <span className="flex-1 truncate" title={tab.url ?? ''}>
                                  {label}
                                </span>
                                {isBound && (
                                  <span className="material-symbols-outlined text-[14px] text-[var(--color-brand)]">
                                    check
                                  </span>
                                )}
                              </button>
                            )
                          })}
                        </div>
                      )
                    })()}
                    {boundTabId != null && (
                      <div className="mt-1.5 flex items-center justify-end">
                        <button
                          type="button"
                          onClick={handleUnbindTab}
                          className="inline-flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                        >
                          <span className="material-symbols-outlined text-[12px]">link_off</span>
                          {t('debug.qa.bindTab.unbind')}
                        </button>
                      </div>
                    )}
                  </div>
                )}
              </div>

              <div className="relative" ref={protoBindRef}>
                <button
                  type="button"
                  onClick={() => {
                    setCredPanelOpen(false)
                    setTabBindOpen(false)
                    setProtoBindOpen((v) => !v)
                  }}
                  className={
                    'inline-flex items-center gap-1 px-2 py-1 text-[11px] rounded border ' +
                    (protoTabId != null || protoFigmaUrl
                      ? 'border-[var(--color-success)]/50 bg-[var(--color-success)]/10 text-[var(--color-success)]'
                      : 'border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]')
                  }
                  title={t('debug.qa.prototypeRef.button')}
                >
                  <span className="material-symbols-outlined text-[14px]">design_services</span>
                  {t('debug.qa.prototypeRef.button')}
                  {protoTabId != null && (
                    <span className="ml-0.5 inline-flex items-center gap-0.5 rounded-full bg-[var(--color-success)]/15 px-1.5 py-px text-[10px] font-mono">
                      #{protoTabId}
                    </span>
                  )}
                  {protoFigmaUrl && (
                    <span className="ml-0.5 inline-flex items-center gap-0.5 rounded-full bg-[var(--color-success)]/15 px-1.5 py-px text-[10px]">
                      Figma
                    </span>
                  )}
                  <span className="material-symbols-outlined text-[14px]">
                    {protoBindOpen ? 'expand_less' : 'expand_more'}
                  </span>
                </button>
                {protoBindOpen && (
                  <div className="absolute z-20 mt-1.5 min-w-[300px] max-w-[460px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] p-2 shadow-md">
                    <div className="mb-1.5 flex items-center justify-between gap-2">
                      <div className="text-[11px] text-[var(--color-text-tertiary)]">
                        {t('debug.qa.prototypeRef.hint')}
                      </div>
                      <button
                        type="button"
                        onClick={() => void refreshProtoTabsForBinder()}
                        className="inline-flex items-center justify-center rounded border border-[var(--color-border)] p-0.5 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
                        title={t('debug.qa.bindTab.refresh')}
                      >
                        <span className="material-symbols-outlined text-[12px]">refresh</span>
                      </button>
                    </div>
                    <div className="mb-2 border-b border-[var(--color-border)] pb-2">
                      <div className="mb-1 text-[11px] text-[var(--color-text-tertiary)]">
                        {t('debug.qa.prototypeRef.figmaHint')}
                      </div>
                      {protoFigmaUrl ? (
                        <div className="flex items-center gap-1.5 rounded border border-[var(--color-success)]/40 bg-[var(--color-success)]/10 px-2 py-1 text-[11px] text-[var(--color-success)]">
                          <span className="material-symbols-outlined text-[13px]">check_circle</span>
                          <span className="flex-1 truncate" title={protoFigmaUrl}>
                            {protoFigmaUrl}
                          </span>
                        </div>
                      ) : (
                        <div className="flex items-center gap-1.5">
                          <input
                            value={protoFigmaDraft}
                            onChange={(e) => setProtoFigmaDraft(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter') {
                                e.preventDefault()
                                handleBindProtoFigma()
                              }
                            }}
                            placeholder={t('debug.qa.prototypeRef.figmaPlaceholder')}
                            className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-accent)]"
                          />
                          <button
                            type="button"
                            onClick={handleBindProtoFigma}
                            disabled={!protoFigmaDraft.trim()}
                            className="inline-flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-40"
                          >
                            <span className="material-symbols-outlined text-[12px]">link</span>
                            {t('debug.qa.prototypeRef.figmaBind')}
                          </button>
                        </div>
                      )}
                    </div>
                    {(() => {
                      const tabs = refreshedProtoTabs ?? dockTabsFromPanel
                      if (tabs.length === 0) {
                        return (
                          <div className="text-[11px] italic text-[var(--color-text-tertiary)] py-1">
                            {t('debug.qa.prototypeRef.empty')}
                          </div>
                        )
                      }
                      return (
                        <div className="flex flex-col gap-1 max-h-[220px] overflow-auto">
                          {tabs.map((tab) => {
                            const isBound = tab.id === protoTabId
                            const label = tab.title?.trim() || tab.url?.trim() || `(tab ${tab.id})`
                            return (
                              <button
                                key={tab.id}
                                type="button"
                                onClick={() => handleBindProto(tab.id)}
                                className={
                                  'flex w-full items-center gap-2 rounded px-2 py-1 text-left text-[11px] transition-colors ' +
                                  (isBound
                                    ? 'border border-[var(--color-success)]/40 bg-[var(--color-success)]/10 text-[var(--color-success)]'
                                    : 'border border-transparent hover:border-[var(--color-border)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-primary)]')
                                }
                              >
                                <span className="font-mono text-[10px] text-[var(--color-text-tertiary)]">
                                  #{tab.id}
                                </span>
                                <span className="flex-1 truncate" title={tab.url ?? ''}>
                                  {label}
                                </span>
                                {isBound && (
                                  <span className="material-symbols-outlined text-[14px] text-[var(--color-success)]">
                                    check
                                  </span>
                                )}
                              </button>
                            )
                          })}
                        </div>
                      )
                    })()}
                    {(protoTabId != null || protoFigmaUrl) && (
                      <div className="mt-1.5 flex items-center justify-end">
                        <button
                          type="button"
                          onClick={handleUnbindProto}
                          className="inline-flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                        >
                          <span className="material-symbols-outlined text-[12px]">link_off</span>
                          {t('debug.qa.prototypeRef.unbind')}
                        </button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          )}

          {isHeroComposer && codingMode !== 'designer' && input.trim().length === 0 && promptSuggestions.length > 0 && (
            <div className="mb-2 flex flex-wrap gap-1.5">
              {promptSuggestions.map((s) => (
                <button
                  key={s.text}
                  type="button"
                  title={s.description}
                  onClick={() => {
                    setInput(s.text)
                    textareaRef.current?.focus()
                  }}
                  className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface-raised)] px-3 py-1 text-[11px] text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent)] hover:text-[var(--color-text-primary)]"
                >
                  {s.text}
                </button>
              ))}
            </div>
          )}
          {isHeroComposer ? (

            <div className="flex flex-1 items-start gap-3">
              <textarea
                ref={textareaRef}
                data-role="chat-composer"
                value={input}
                onChange={handleInputChange}
                onKeyDown={handleKeyDown}
                onCompositionStart={() => { composingRef.current = true }}
                onCompositionEnd={() => { composingRef.current = false }}
                onPaste={handlePaste}
                placeholder={composerPlaceholder}
                disabled={isWorkspaceMissing}
                rows={1}
                className="min-h-[54px] w-full flex-1 resize-none border-none bg-transparent py-1 text-[12px] leading-relaxed text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] disabled:opacity-50"
              />
            </div>
          ) : (

            <textarea
              ref={textareaRef}
              data-role="chat-composer"
              value={input}
              onChange={handleInputChange}
              onKeyDown={handleKeyDown}
              onCompositionStart={() => { composingRef.current = true }}
              onCompositionEnd={() => { composingRef.current = false }}
              onPaste={handlePaste}
              placeholder={composerPlaceholder}
              disabled={isWorkspaceMissing}
              rows={1}
              className={`w-full min-h-[80px] resize-none bg-transparent py-1 ${codingMode === 'designer' ? 'pb-1' : 'pb-9'} text-[12px] leading-relaxed text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] disabled:opacity-50`}
            />
          )}

          {}
          <div className={isHeroComposer
            ? 'flex items-start justify-between gap-2'
            : codingMode === 'designer'
              ? 'flex items-start justify-between gap-2 px-3 pb-2 pt-1'
              : 'absolute bottom-0 left-0 right-0 flex items-center justify-between px-3 py-2'}>
            {}
            <div
              className={`flex min-w-0 flex-1 gap-1.5 ${
                codingMode === 'designer' && !isMemberSession
                  ? 'flex-col items-start'
                  : 'flex-wrap items-center'
              }`}
            >
              {!isMemberSession && (
                <>
                  <div className="flex flex-wrap items-center gap-1.5">
                    <CodingModeSelector />
                    {activeTabId && (
                      designerMediaType && designerSelectedId ? (
                        <ModelSelector
                          value={designerMediaModel}
                          onChange={(id) =>
                            designerSetParam(activeTabId, designerSelectedId, 'model', id)
                          }
                          requiredType={designerMediaType}
                          modelPool={designerMediaPool}
                          disabled={isActive}
                        />
                      ) : (
                        <ModelSelector runtimeKey={activeTabId} disabled={isActive} />
                      )
                    )}
                  </div>
                  {codingMode === 'designer' && activeTabId && (
                    <DesignerInlineControls sessionId={activeTabId} />
                  )}
                </>
              )}
            </div>

            {}
            <div className="flex items-center gap-1">
              {!isMemberSession && (
                <TokenUsageRing sessionId={activeTabId ?? null} size={14} />
              )}
              {(() => {
                const useAccent = !isMemberSession && !!modeAccent
                let bgIdle: string
                let bgHover: string
                let fg: string
                if (actAsStopButton) {
                  if (useAccent && modeAccent) {
                    bgIdle = modeAccent.accent
                    bgHover = modeAccent.accentHover
                    fg = modeAccent.onAccent
                  } else if (isPlanMode) {
                    bgIdle = 'var(--color-plan-accent)'
                    bgHover = 'var(--color-plan-accent-hover)'
                    fg = 'var(--color-on-plan-accent)'
                  } else {
                    bgIdle = 'var(--color-error-container)'
                    bgHover = 'var(--color-error-container)'
                    fg = 'var(--color-on-error-container)'
                  }
                } else if (useAccent && modeAccent) {
                  bgIdle = modeAccent.accent
                  bgHover = modeAccent.accentHover
                  fg = modeAccent.onAccent
                } else if (isPlanMode && !isMemberSession) {
                  bgIdle = 'var(--color-plan-accent)'
                  bgHover = 'var(--color-plan-accent-hover)'
                  fg = 'var(--color-on-plan-accent)'
                } else {
                  bgIdle = 'var(--color-text-primary)'
                  bgHover = 'var(--color-text-primary)'
                  fg = 'var(--color-surface)'
                }
                const isDisabled = actAsStopButton ? showStopping : !canSubmit
                const bg = sendButtonHover && !isDisabled ? bgHover : bgIdle
                return (
                  <button
                    onClick={actAsStopButton ? handleStopClick : handleSubmit}
                    onMouseEnter={() => setSendButtonHover(true)}
                    onMouseLeave={() => setSendButtonHover(false)}
                    disabled={isDisabled}
                    aria-label={
                      actAsStopButton
                        ? showStopping
                          ? t('chat.stopping')
                          : t('chat.stopTitle')
                        : showNoModelBanner
                          ? t('chat.noModel.sendTooltip')
                          : isMemberSession ? t('common.send') : t('common.run')
                    }
                    title={
                      actAsStopButton
                        ? showStopping
                          ? t('chat.stopping')
                          : t('chat.stopTitle')
                        : showNoModelBanner
                          ? t('chat.noModel.sendTooltip')
                          : isMemberSession ? t('common.send') : t('common.run')
                    }
                    className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full shadow-[var(--shadow-button-primary)] transition-all disabled:cursor-not-allowed disabled:opacity-50"
                    style={{ backgroundColor: bg, color: fg }}
                  >
                    {actAsStopButton ? (
                      showStopping ? (
                        <svg
                          width="13"
                          height="13"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="3"
                          strokeLinecap="round"
                          className="animate-spin"
                          aria-hidden
                        >
                          <path d="M12 3a9 9 0 1 0 9 9" />
                        </svg>
                      ) : (
                        <svg
                          width="12"
                          height="12"
                          viewBox="0 0 24 24"
                          fill="currentColor"
                          aria-hidden
                        >
                          <rect x="5" y="5" width="14" height="14" rx="2" />
                        </svg>
                      )
                    ) : (
                      <svg
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.6"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        aria-hidden
                      >
                        <line x1="12" y1="20" x2="12" y2="5" />
                        <polyline points="5,12 12,5 19,12" />
                      </svg>
                    )}
                  </button>
                )
              })()}
            </div>
          </div>
        </div>

        {!isMemberSession && (
          <div className="mt-2 px-1">
            {hasMessages ? (
              <ProjectContextChip
                workDir={resolvedWorkDir}
                repoName={gitInfo?.repoName || null}
                branch={gitInfo?.branch || null}
              />
            ) : (
              <DirectoryPicker
                value={resolvedWorkDir || ''}
                onChange={async (newWorkDir) => {
                  if (!activeTabId) return
                  useSessionStore.getState().setUserPinnedSessionWorkDir(newWorkDir)
                  const oldId = activeTabId
                  const { deleteSession, createSession } = useSessionStore.getState()
                  const { replaceTabSession } = useTabStore.getState()
                  const { disconnectSession, connectToSession, setSessionPermissionMode } = useChatStore.getState()
                  const newId = await createSession(newWorkDir)
                  useSessionRuntimeStore.getState().moveSelection(oldId, newId)
                  disconnectSession(oldId)
                  replaceTabSession(oldId, newId)
                  connectToSession(newId)
                  setSessionPermissionMode(newId, useSettingsStore.getState().permissionMode)
                  deleteSession(oldId).catch(() => {})
                }}
              />
            )}
          </div>
        )}
      </div>
    </div>
  )
}
