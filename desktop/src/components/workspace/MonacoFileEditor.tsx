import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import Editor, { type OnMount } from '@monaco-editor/react'
import type * as MonacoNs from 'monaco-editor'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useLspStore } from '../../stores/lspStore'
import { lspBridge } from '../../lib/lspBridge'
import {
  AI_FRESH_WINDOW_MS,
  nameOf,
  useWorkspaceFilesStore,
} from '../../stores/workspaceFilesStore'
import { applyAiDecorations } from '../../lib/aiDecorations'
import type { LspDiagnostic, LspPosition } from '../../types/lsp'
import { MonacoDiffOverlay } from './MonacoDiffOverlay'
import { MediaPreview, classifyMedia } from './MediaPreview'

import '../../lib/monacoSetup'

type Props = {
  workDir: string
}

function joinAbsPath(root: string, rel: string): string {
  if (!rel) return root
  if (root.endsWith('/') || root.endsWith('\\')) return `${root}${rel}`
  if (root.includes('\\') && !root.includes('/')) return `${root}\\${rel.replace(/\//g, '\\')}`
  return `${root}/${rel}`
}

function pathToUri(absPath: string): string {
  let p = absPath.replace(/\\/g, '/')
  if (!p.startsWith('/')) p = '/' + p
  return `file://${p}`
}

function languageIdFor(filename: string): string {
  const ext = filename.split('.').pop()?.toLowerCase() ?? ''
  switch (ext) {
    case 'ts':
      return 'typescript'
    case 'tsx':
      return 'typescript'
    case 'js':
    case 'mjs':
    case 'cjs':
      return 'javascript'
    case 'jsx':
      return 'javascript'
    case 'rs':
      return 'rust'
    case 'py':
    case 'pyi':
      return 'python'
    case 'go':
      return 'go'
    case 'java':
      return 'java'
    case 'kt':
      return 'kotlin'
    case 'swift':
      return 'swift'
    case 'cpp':
    case 'cc':
    case 'cxx':
    case 'hpp':
    case 'hxx':
      return 'cpp'
    case 'c':
    case 'h':
      return 'c'
    case 'cs':
      return 'csharp'
    case 'rb':
      return 'ruby'
    case 'php':
      return 'php'
    case 'json':
    case 'jsonc':
      return 'json'
    case 'md':
    case 'markdown':
      return 'markdown'
    case 'html':
    case 'htm':
      return 'html'
    case 'css':
      return 'css'
    case 'scss':
      return 'scss'
    case 'less':
      return 'less'
    case 'yaml':
    case 'yml':
      return 'yaml'
    case 'toml':
      return 'plaintext'
    case 'xml':
      return 'xml'
    case 'sh':
    case 'bash':
    case 'zsh':
      return 'shell'
    case 'sql':
      return 'sql'
    case 'dockerfile':
      return 'dockerfile'
    case 'lua':
      return 'lua'
    default:
      return 'plaintext'
  }
}

function lspPosToMonaco(pos: LspPosition): MonacoNs.IPosition {
  return { lineNumber: pos.line + 1, column: pos.character + 1 }
}

function monacoPosToLsp(pos: MonacoNs.Position): LspPosition {
  return { line: pos.lineNumber - 1, character: pos.column - 1 }
}

function severityFor(
  monaco: typeof MonacoNs,
  raw: number | undefined,
): MonacoNs.MarkerSeverity {
  switch (raw) {
    case 1:
      return monaco.MarkerSeverity.Error
    case 2:
      return monaco.MarkerSeverity.Warning
    case 3:
      return monaco.MarkerSeverity.Info
    case 4:
      return monaco.MarkerSeverity.Hint
    default:
      return monaco.MarkerSeverity.Info
  }
}

function lspToMarker(
  monaco: typeof MonacoNs,
  diag: LspDiagnostic,
): MonacoNs.editor.IMarkerData | null {
  if (!diag?.range?.start || !diag?.range?.end) return null
  const start = lspPosToMonaco(diag.range.start)
  const end = lspPosToMonaco(diag.range.end)
  return {
    severity: severityFor(monaco, diag.severity),
    startLineNumber: start.lineNumber,
    startColumn: start.column,
    endLineNumber: end.lineNumber,
    endColumn: Math.max(start.column + 1, end.column),
    message: diag.message ?? '',
    source: diag.source ?? undefined,
    code: diag.code != null ? String(diag.code) : undefined,
  }
}

function flattenHover(result: unknown): string | null {
  if (!result || typeof result !== 'object') return null
  const r = result as { contents?: unknown }
  const contents = r.contents
  if (!contents) return null
  const collect = (node: unknown): string => {
    if (!node) return ''
    if (typeof node === 'string') return node
    if (Array.isArray(node)) return node.map(collect).filter(Boolean).join('\n\n')
    if (typeof node === 'object') {
      const obj = node as { value?: unknown }
      if (typeof obj.value === 'string') return obj.value
    }
    return ''
  }
  const text = collect(contents).trim()
  return text.length > 0 ? text : null
}

type LspCompletionItem = {
  label: string
  kind?: number
  detail?: string
  documentation?: string | { kind?: string; value?: string }
  insertText?: string
  filterText?: string
  sortText?: string
  textEdit?: {
    newText?: string
    range?: { start: LspPosition; end: LspPosition }
  }
}

function flattenCompletions(result: unknown): LspCompletionItem[] {
  if (!result) return []
  if (Array.isArray(result)) return result as LspCompletionItem[]
  const r = result as { items?: unknown[] }
  if (Array.isArray(r.items)) return r.items as LspCompletionItem[]
  return []
}

function docToString(doc: LspCompletionItem['documentation']): string | undefined {
  if (!doc) return undefined
  if (typeof doc === 'string') return doc
  if (typeof doc === 'object' && typeof doc.value === 'string') return doc.value
  return undefined
}

const COMPLETION_KIND_TO_MONACO_KIND: Record<number, number> = {

  1: 18,
  2: 0,
  3: 1,
  4: 2,
  5: 4,
  6: 4,
  7: 5,
  8: 7,
  9: 8,
  10: 9,
  11: 12,
  12: 13,
  13: 15,
  14: 17,
  15: 26,
  16: 19,
  17: 20,
  18: 21,
  19: 22,
  20: 16,
  21: 14,
  22: 23,
  23: 10,
  24: 11,
  25: 24,
}

export function MonacoFileEditor({ workDir }: Props) {
  void workDir
  const t = useTranslation()
  const theme = useUIStore((s) => s.theme)
  const addToast = useUIStore((s) => s.addToast)
  const root = useWorkspaceFilesStore((s) => s.root)
  const activeTab = useWorkspaceFilesStore((s) => s.activeTab)
  const buffer = useWorkspaceFilesStore((s) => {
    if (!s.root || !s.activeTab) return undefined
    return s.files[`${s.root}::${s.activeTab}`]
  })
  const lastSeen = useWorkspaceFilesStore((s) => {
    if (!s.activeTab) return undefined
    return s.lastSeenContent[s.activeTab]
  })
  const aiModifiedTs = useWorkspaceFilesStore((s) => {
    if (!s.activeTab) return undefined
    return s.aiModifiedAt[s.activeTab]
  })
  const externalChangedTs = useWorkspaceFilesStore((s) => {
    if (!s.activeTab) return undefined
    return s.externalChanged[s.activeTab]
  })
  const updateDraft = useWorkspaceFilesStore((s) => s.updateDraft)
  const saveFile = useWorkspaceFilesStore((s) => s.saveFile)
  const reloadFile = useWorkspaceFilesStore((s) => s.reloadFile)
  const acknowledgeExternalChange = useWorkspaceFilesStore(
    (s) => s.acknowledgeExternalChange,
  )

  const fileUri = useMemo(() => {
    if (!root || !activeTab) return null
    return pathToUri(joinAbsPath(root, activeTab))
  }, [root, activeTab])

  const languageId = useMemo(() => {
    if (!activeTab) return 'plaintext'
    return languageIdFor(nameOf(activeTab))
  }, [activeTab])

  const editorRef = useRef<MonacoNs.editor.IStandaloneCodeEditor | null>(null)
  const monacoRef = useRef<typeof MonacoNs | null>(null)

  const ctxRef = useRef<{ uri: string | null; languageId: string }>({
    uri: null,
    languageId: 'plaintext',
  })
  ctxRef.current.uri = fileUri
  ctxRef.current.languageId = languageId

  useEffect(() => {
    if (!fileUri || !buffer || buffer.isBinary) return
    const text = buffer.draft ?? ''
    void lspBridge.didOpen({ uri: fileUri, languageId, text })
    return () => {
      void lspBridge.didClose(fileUri)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fileUri, buffer?.isBinary])

  useEffect(() => {
    if (!fileUri || !buffer || buffer.isBinary) return
    lspBridge.didChange({
      uri: fileUri,
      languageId,
      text: buffer.draft ?? '',
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fileUri, buffer?.draft, buffer?.isBinary])

  const handleSave = useCallback(async () => {
    if (!activeTab || !buffer || buffer.isBinary) return
    if (!buffer.isDirty) return
    try {
      await saveFile(activeTab)
      if (fileUri) {
        const latest =
          useWorkspaceFilesStore.getState().files[`${root}::${activeTab}`]
        void lspBridge.didSave({
          uri: fileUri,
          languageId,
          text: latest?.original ?? buffer.draft ?? '',
        })
      }
      addToast({ type: 'success', message: t('files.saveSuccess') })
    } catch (err) {
      addToast({
        type: 'error',
        message: t('files.saveError', {
          message: err instanceof Error ? err.message : String(err),
        }),
      })
    }
  }, [activeTab, addToast, buffer, fileUri, languageId, root, saveFile, t])

  const currentDiagnostics = useLspStore((s) =>
    fileUri ? s.diagnosticsByUri[fileUri]?.diagnostics : undefined,
  )
  useEffect(() => {
    const editor = editorRef.current
    const monaco = monacoRef.current
    if (!editor || !monaco) return
    const model = editor.getModel()
    if (!model) return
    const markers: MonacoNs.editor.IMarkerData[] = []
    if (currentDiagnostics) {
      for (const diag of currentDiagnostics) {
        const m = lspToMarker(monaco, diag)
        if (m) markers.push(m)
      }
    }
    monaco.editor.setModelMarkers(model, 'lsp', markers)
  }, [currentDiagnostics])

  const [pendingDiff, setPendingDiff] = useState<{
    relPath: string
    previousContent: string
    currentContent: string
    lineCount: number
    landedAt: number
  } | null>(null)
  const [diffOverlayOpen, setDiffOverlayOpen] = useState(false)

  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!pendingDiff) return
    const interval = window.setInterval(() => setNow(Date.now()), 1_000)
    return () => window.clearInterval(interval)
  }, [pendingDiff])

  useEffect(() => {
    if (!pendingDiff) return
    if (now - pendingDiff.landedAt >= AI_FRESH_WINDOW_MS) {
      setPendingDiff(null)
    }
  }, [now, pendingDiff])

  useEffect(() => {
    const editor = editorRef.current
    const monaco = monacoRef.current
    if (!editor || !monaco || !aiModifiedTs || !activeTab) return
    if (!buffer || buffer.isBinary) return
    if (lastSeen === undefined) return
    const currentContent = buffer.original
    if (currentContent === lastSeen) return
    const result = applyAiDecorations({
      monaco,
      editor,
      previousContent: lastSeen,
      currentContent,
    })
    if (result.changedLineCount > 0) {
      setPendingDiff({
        relPath: activeTab,
        previousContent: lastSeen,
        currentContent,
        lineCount: result.changedLineCount,
        landedAt: Date.now(),
      })
    }
    useWorkspaceFilesStore.getState().snapshotLastSeen(activeTab, currentContent)
  }, [aiModifiedTs, buffer, lastSeen, activeTab])

  useEffect(() => {
    setPendingDiff(null)
    setDiffOverlayOpen(false)
  }, [activeTab])

  const providersRegistered = useRef<Set<string>>(new Set())

  const onMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor
    monacoRef.current = monaco

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void handleSave()
    })

    const ensureProviders = (lang: string) => {
      if (providersRegistered.current.has(lang)) return
      providersRegistered.current.add(lang)

      monaco.languages.registerHoverProvider(lang, {
        provideHover: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.hover({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
            })
            const text = flattenHover(result)
            if (!text) return null
            return {
              contents: [{ value: text }],
            }
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerCompletionItemProvider(lang, {
        triggerCharacters: ['.', ':', '/', '@', '<', '"', "'", ' '],
        provideCompletionItems: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return { suggestions: [] }
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.completion({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
            })
            const items = flattenCompletions(result)
            const word = model.getWordUntilPosition(position)
            const range: MonacoNs.IRange = {
              startLineNumber: position.lineNumber,
              endLineNumber: position.lineNumber,
              startColumn: word.startColumn,
              endColumn: word.endColumn,
            }
            return {
              suggestions: items.map((item) => ({
                label: item.label,
                kind:
                  (item.kind != null
                    ? COMPLETION_KIND_TO_MONACO_KIND[item.kind]
                    : monaco.languages.CompletionItemKind.Text) ??
                  monaco.languages.CompletionItemKind.Text,
                insertText: item.textEdit?.newText ?? item.insertText ?? item.label,
                detail: item.detail,
                documentation: docToString(item.documentation),
                sortText: item.sortText,
                filterText: item.filterText,
                range,
              })),
            }
          } catch {
            return { suggestions: [] }
          }
        },
      })
    }

    ensureProviders(languageId)

    editor.onDidChangeModel(() => {
      const model = editor.getModel()
      if (model) ensureProviders(model.getLanguageId())
    })
  }, [handleSave, languageId])

  if (!root || !activeTab) {
    return (
      <div className="flex h-full items-center justify-center px-4 text-center text-xs text-[var(--color-text-tertiary)]">
        {t('files.noFileSelected')}
      </div>
    )
  }

  if (!buffer || buffer.loading) {
    return (
      <div className="flex h-full items-center justify-center px-4 text-xs text-[var(--color-text-tertiary)]">
        {t('rightSidebar.loading')}
      </div>
    )
  }

  if (buffer.error) {
    return (
      <div className="flex h-full items-center justify-center px-4 text-center text-xs text-[var(--color-danger)]">
        {t('files.errorLoading', { message: buffer.error })}
      </div>
    )
  }

  if (buffer.isBinary) {
    const kind = classifyMedia(nameOf(activeTab), buffer.mimeType)
    if (kind !== 'unknown') {
      return (
        <MediaPreview
          content={buffer.original}
          encoding={buffer.encoding}
          mimeType={buffer.mimeType}
          fileName={nameOf(activeTab)}
          relPath={activeTab}
          sizeBytes={buffer.sizeBytes}
        />
      )
    }
    return (
      <div className="flex h-full flex-col">
        <EditorHeader name={nameOf(activeTab)} relPath={activeTab} />
        <div className="flex flex-1 items-center justify-center px-4 text-center text-xs text-[var(--color-text-tertiary)]">
          {t('files.binaryNotPreviewable')}
        </div>
      </div>
    )
  }

  return (
    <div data-workspace-editor className="relative flex h-full min-h-0 flex-col">
      <EditorHeader
        name={nameOf(activeTab)}
        relPath={activeTab}
        isDirty={buffer.isDirty}
        saving={buffer.saving}
        onSave={handleSave}
        saveError={buffer.saveError}
      />
      {externalChangedTs !== undefined && (
        <div className="flex flex-shrink-0 items-center gap-2 border-b border-[var(--color-warning)]/30 bg-[var(--color-warning)]/10 px-2 py-1.5 text-[11px] text-[var(--color-text-primary)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-warning)]">
            error
          </span>
          <span className="flex-1">{t('files.externalChanged')}</span>
          <button
            type="button"
            onClick={() => {
              void reloadFile(activeTab)
              acknowledgeExternalChange(activeTab)
            }}
            className="rounded px-2 py-0.5 text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('files.externalChangedReload')}
          </button>
          <button
            type="button"
            onClick={() => acknowledgeExternalChange(activeTab)}
            className="rounded px-2 py-0.5 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            {t('files.externalChangedDismiss')}
          </button>
        </div>
      )}
      {pendingDiff && pendingDiff.relPath === activeTab && (
        <div className="flex flex-shrink-0 items-center gap-2 border-b border-[var(--color-success)]/30 bg-[var(--color-success)]/10 px-2 py-1.5 text-[11px] text-[var(--color-text-primary)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-success)]">
            auto_awesome
          </span>
          <span className="flex-1">
            {t('files.aiModifiedToast', { count: pendingDiff.lineCount })}
          </span>
          <button
            type="button"
            onClick={() => setDiffOverlayOpen(true)}
            className="rounded px-2 py-0.5 text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('files.aiModifiedToastViewDiff')}
          </button>
          <button
            type="button"
            onClick={() => setPendingDiff(null)}
            className="rounded px-2 py-0.5 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            {t('files.externalChangedDismiss')}
          </button>
        </div>
      )}
      <div className="relative min-h-0 flex-1">
        <Editor
          path={activeTab}
          theme={theme === 'dark' ? 'vs-dark' : 'vs'}
          language={languageId}
          value={buffer.draft}
          onMount={onMount}
          onChange={(value) => {
            if (typeof value === 'string') {
              updateDraft(activeTab, value)
            }
          }}
          options={{
            automaticLayout: true,
            fontSize: 13,
            lineHeight: 20,
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            smoothScrolling: true,
            renderWhitespace: 'selection',
            tabSize: 2,
            wordWrap: 'off',
            wordBasedSuggestions: 'currentDocument',
            quickSuggestions: { other: true, comments: false, strings: true },
            suggestOnTriggerCharacters: true,
            fixedOverflowWidgets: true,
          }}
        />
        {diffOverlayOpen && pendingDiff && pendingDiff.relPath === activeTab && (
          <MonacoDiffOverlay
            previousContent={pendingDiff.previousContent}
            currentContent={pendingDiff.currentContent}
            languageId={languageId}
            onClose={() => setDiffOverlayOpen(false)}
          />
        )}
      </div>
    </div>
  )
}

function EditorHeader({
  name,
  relPath,
  isDirty,
  saving,
  onSave,
  saveError,
}: {
  name: string
  relPath: string
  isDirty?: boolean
  saving?: boolean
  onSave?: () => void
  saveError?: string
}) {
  const t = useTranslation()
  return (
    <div className="flex h-7 flex-shrink-0 items-center gap-2 border-b border-[var(--color-border)] bg-[var(--color-surface-container)] px-2 text-xs">
      <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
        edit_note
      </span>
      <span
        className="truncate font-medium text-[var(--color-text-primary)]"
        title={relPath}
      >
        {name}
      </span>
      {isDirty && (
        <span
          className="ml-1 inline-block size-1.5 rounded-full bg-[var(--color-accent)]"
          title={t('files.unsavedChanges')}
          aria-label={t('files.unsavedChanges')}
        />
      )}
      {saveError && (
        <span
          className="ml-2 truncate text-[10px] text-[var(--color-danger)]"
          title={saveError}
        >
          {saveError}
        </span>
      )}
      <div className="ml-auto flex items-center gap-1">
        {onSave && (
          <button
            type="button"
            onClick={onSave}
            disabled={saving || !isDirty}
            className="flex h-5 items-center gap-1 rounded px-2 text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40 disabled:hover:bg-transparent"
            title="Ctrl/Cmd + S"
          >
            <span className="material-symbols-outlined text-[14px]">save</span>
            {saving ? t('common.loading') : t('common.save')}
          </button>
        )}
      </div>
    </div>
  )
}
