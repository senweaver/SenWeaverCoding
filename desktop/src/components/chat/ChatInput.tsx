import { useState, useRef, useEffect, useCallback, useMemo } from 'react'
import { useTranslation } from '../../i18n'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useUIStore } from '../../stores/uiStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useSettingsStore } from '../../stores/settingsStore'
import { useSessionRuntimeStore } from '../../stores/sessionRuntimeStore'
import { useTeamStore } from '../../stores/teamStore'
import { sessionsApi } from '../../api/sessions'
import { CodingModeSelector } from '../controls/CodingModeSelector'
import { ModelSelector } from '../controls/ModelSelector'
import type { AttachmentRef } from '../../types/chat'
import { AttachmentGallery } from './AttachmentGallery'
import { ProjectContextChip } from '../shared/ProjectContextChip'
import { DirectoryPicker } from '../shared/DirectoryPicker'
import { FileSearchMenu, type FileSearchMenuHandle } from './FileSearchMenu'
import { LocalSlashCommandPanel, type LocalSlashCommandName } from './LocalSlashCommandPanel'
import { ReviewCard } from './ReviewCard'
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

export function ChatInput({ variant = 'default' }: ChatInputProps) {
  const t = useTranslation()
  const [input, setInput] = useState('')
  const [attachments, setAttachments] = useState<Attachment[]>([])
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
  const sessionState = useChatStore((s) => activeTabId ? s.sessions[activeTabId] : undefined)
  const chatState = sessionState?.chatState ?? 'idle'
  const stopRequested = sessionState?.stopRequested ?? false
  const slashCommands = sessionState?.slashCommands ?? []
  const composerPrefill = sessionState?.composerPrefill ?? null
  const codingMode = useSettingsStore((s) => s.codingMode)
  const isPlanMode = codingMode === 'plan'
  const isDebugMode = codingMode === 'debug'
  const credentialsList = useCredentialsStore((s) => s.credentials)
  const credentialsHasFetched = useCredentialsStore((s) => s.hasFetched)
  const credentialsIsLoading = useCredentialsStore((s) => s.isLoading)
  const fetchCredentials = useCredentialsStore((s) => s.fetchAll)
  const [credPanelOpen, setCredPanelOpen] = useState(false)
  const credPanelRef = useRef<HTMLDivElement>(null)
  const activeSession = useSessionStore((state) => activeTabId ? state.sessions.find((session) => session.id === activeTabId) ?? null : null)
  const memberInfo = useTeamStore((s) => activeTabId ? s.getMemberBySessionId(activeTabId) : null)
  const [gitInfo, setGitInfo] = useState<GitInfo | null>(null)
  const hasMessages = useChatStore((s) => activeTabId ? (s.sessions[activeTabId]?.messages?.length ?? 0) > 0 : false)

  const isMemberSession = !!memberInfo
  const isActive = chatState !== 'idle'
  const [stopCooldown, setStopCooldown] = useState(false)
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
    if (!isDebugMode) {
      setCredPanelOpen(false)
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
  const canSubmit = !isWorkspaceMissing && (input.trim().length > 0 || (!isMemberSession && attachments.length > 0))
  const actAsStopButton = !isMemberSession && isActive && !canSubmit
  const isHeroComposer = variant === 'hero' && !isMemberSession
  const resolvedWorkDir = activeSession?.workDir || gitInfo?.workDir || undefined

  useEffect(() => {
    textareaRef.current?.focus()
  }, [isActive])

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

    sendMessage(activeTabId!, text, attachmentPayload)
    setInput('')
    setAttachments([])
    setSlashMenuOpen(false)
    setFileSearchOpen(false)
    setLocalSlashPanel(null)
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
    !!sessionState?.pendingPermission &&
    (sessionState.pendingPermission.toolName === 'ask_question' ||
      sessionState.pendingPermission.toolName === 'AskUserQuestion')

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

          {!isMemberSession && isDebugMode && (
            <div className="mb-1.5" ref={credPanelRef}>
              <button
                type="button"
                onClick={() => setCredPanelOpen((v) => !v)}
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
                <div className="mt-1.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] p-2">
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
              className="w-full min-h-[80px] resize-none bg-transparent py-1 pb-9 text-[12px] leading-relaxed text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] disabled:opacity-50"
            />
          )}

          {}
          <div className={isHeroComposer
            ? 'flex items-center justify-between'
            : 'absolute bottom-0 left-0 right-0 flex items-center justify-between px-3 py-2'}>
            {}
            <div className="flex items-center gap-1.5">
              {!isMemberSession && (
                <>
                  <CodingModeSelector />
                  {activeTabId && (
                    <ModelSelector runtimeKey={activeTabId} disabled={isActive} />
                  )}
                </>
              )}
            </div>

            {}
            <div className="flex items-center gap-1">
              {!isMemberSession && (
                <TokenUsageRing sessionId={activeTabId ?? null} size={14} />
              )}
              <button
                onClick={actAsStopButton ? handleStopClick : handleSubmit}
                disabled={
                  actAsStopButton
                    ? showStopping
                    : !canSubmit
                }
                aria-label={
                  actAsStopButton
                    ? showStopping
                      ? t('chat.stopping')
                      : t('chat.stopTitle')
                    : isMemberSession ? t('common.send') : t('common.run')
                }
                title={
                  actAsStopButton
                    ? showStopping
                      ? t('chat.stopping')
                      : t('chat.stopTitle')
                    : isMemberSession ? t('common.send') : t('common.run')
                }
                className={`flex h-5 w-5 flex-shrink-0 items-center justify-center rounded-full transition-all hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-50 ${
                  actAsStopButton
                    ? isPlanMode
                      ? 'bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)] shadow-[var(--shadow-button-primary)]'
                      : 'bg-[var(--color-error-container)] text-[var(--color-on-error-container)]'
                    : isPlanMode && !isMemberSession
                      ? 'bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)] shadow-[var(--shadow-button-primary)]'
                      : 'bg-[var(--color-text-primary)] text-[var(--color-surface)] shadow-[var(--shadow-button-primary)]'
                }`}
              >
                <span
                  className={`material-symbols-outlined text-[8px] ${
                    showStopping ? 'animate-spin' : ''
                  }`}
                >
                  {actAsStopButton
                    ? showStopping
                      ? 'progress_activity'
                      : 'stop'
                    : 'arrow_upward'}
                </span>
              </button>
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
