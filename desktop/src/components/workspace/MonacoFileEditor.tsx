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
  type MonacoEditOperation,
  type MonacoModelHandle,
} from '../../stores/workspaceFilesStore'
import { applyAiDecorations } from '../../lib/aiDecorations'
import { formatBytes } from '../../lib/formatBytes'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { revealInExplorer } from '../../lib/revealInExplorer'
import { joinWorkspaceAbsPath, workspaceAbsPathToUri } from '../../lib/workspacePath'
import type { LspDiagnostic, LspPosition } from '../../types/lsp'
import { copyTextToClipboard } from '../chat/clipboard'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { MonacoDiffOverlay } from './MonacoDiffOverlay'
import { MediaPreview, classifyMedia } from './MediaPreview'
import { StaticCodeViewer } from './StaticCodeViewer'

import '../../lib/monacoSetup'

const EDITOR_PREFS_STORAGE_KEY = 'sen-workspace-editor-prefs'

const LARGE_FILE_BYTE_THRESHOLD = 2 * 1024 * 1024

const LARGE_FILE_LINE_THRESHOLD = 50_000

const LARGE_FILE_TRUNCATE_LINES = 5_000

type EditorPrefs = {
  wordWrap: boolean
  minimap: boolean
  whitespace: boolean
}

const DEFAULT_PREFS: EditorPrefs = {
  wordWrap: false,
  minimap: false,
  whitespace: false,
}

function readEditorPrefs(): EditorPrefs {
  if (typeof window === 'undefined') return DEFAULT_PREFS
  try {
    const raw = window.localStorage.getItem(EDITOR_PREFS_STORAGE_KEY)
    if (!raw) return DEFAULT_PREFS
    const parsed = JSON.parse(raw) as Partial<EditorPrefs>
    return {
      wordWrap: typeof parsed.wordWrap === 'boolean' ? parsed.wordWrap : DEFAULT_PREFS.wordWrap,
      minimap: typeof parsed.minimap === 'boolean' ? parsed.minimap : DEFAULT_PREFS.minimap,
      whitespace:
        typeof parsed.whitespace === 'boolean' ? parsed.whitespace : DEFAULT_PREFS.whitespace,
    }
  } catch {
    return DEFAULT_PREFS
  }
}

function writeEditorPrefs(next: EditorPrefs) {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(EDITOR_PREFS_STORAGE_KEY, JSON.stringify(next))
  } catch {}
}

function countLines(text: string): number {
  if (!text) return 0
  let n = 1
  for (let i = 0; i < text.length; i += 1) {
    if (text.charCodeAt(i) === 10) n += 1
  }
  return n
}

function isMarkdownExt(filename: string): boolean {
  const ext = filename.split('.').pop()?.toLowerCase() ?? ''
  return ext === 'md' || ext === 'markdown' || ext === 'mdx'
}

type Props = {
  workDir: string
}

const FILENAME_LANGUAGE: Record<string, string> = {
  dockerfile: 'dockerfile',
  containerfile: 'dockerfile',
  '.dockerignore': 'ignore',
  makefile: 'plaintext',
  gnumakefile: 'plaintext',
  rakefile: 'ruby',
  gemfile: 'ruby',
  guardfile: 'ruby',
  capfile: 'ruby',
  brewfile: 'ruby',
  podfile: 'ruby',
  vagrantfile: 'ruby',
  procfile: 'plaintext',
  cmakelists: 'plaintext',
  'cmakelists.txt': 'plaintext',
  'cargo.toml': 'plaintext',
  'cargo.lock': 'plaintext',
  'package.json': 'json',
  'package-lock.json': 'json',
  'tsconfig.json': 'json',
  'jsconfig.json': 'json',
  'composer.json': 'json',
  'composer.lock': 'json',
  'yarn.lock': 'plaintext',
  'pnpm-lock.yaml': 'yaml',
  'docker-compose.yml': 'yaml',
  'docker-compose.yaml': 'yaml',
  '.gitignore': 'ignore',
  '.gitattributes': 'plaintext',
  '.gitconfig': 'ini',
  '.gitmodules': 'ini',
  '.editorconfig': 'ini',
  '.npmrc': 'ini',
  '.yarnrc': 'plaintext',
  '.nvmrc': 'plaintext',
  '.env': 'plaintext',
  '.env.local': 'plaintext',
  '.env.development': 'plaintext',
  '.env.production': 'plaintext',
  '.env.test': 'plaintext',
  '.eslintrc': 'json',
  '.eslintrc.json': 'json',
  '.prettierrc': 'json',
  '.prettierrc.json': 'json',
  '.babelrc': 'json',
  '.babelrc.json': 'json',
  '.bashrc': 'shell',
  '.zshrc': 'shell',
  '.bash_profile': 'shell',
  '.zprofile': 'shell',
  '.profile': 'shell',
  license: 'plaintext',
  'license.md': 'markdown',
  'license.txt': 'plaintext',
  copying: 'plaintext',
  notice: 'plaintext',
  readme: 'plaintext',
  'readme.md': 'markdown',
  'readme.txt': 'plaintext',
  changelog: 'plaintext',
  'changelog.md': 'markdown',
  authors: 'plaintext',
  owners: 'plaintext',
  contributors: 'plaintext',
  contributing: 'plaintext',
  'contributing.md': 'markdown',
  todo: 'plaintext',
  'todo.md': 'markdown',
}

function languageIdFor(filename: string): string {
  if (!filename) return 'plaintext'
  const lower = filename.toLowerCase()
  const byName = FILENAME_LANGUAGE[lower]
  if (byName) return byName
  if (lower.startsWith('dockerfile.') || lower.endsWith('.dockerfile')) {
    return 'dockerfile'
  }
  if (lower.startsWith('makefile.') || lower.endsWith('.mk') || lower.endsWith('.make')) {
    return 'plaintext'
  }
  const dot = lower.lastIndexOf('.')
  const ext = dot >= 0 ? lower.slice(dot + 1) : ''
  switch (ext) {
    case 'ts':
    case 'mts':
    case 'cts':
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
    case 'pyx':
      return 'python'
    case 'go':
      return 'go'
    case 'java':
      return 'java'
    case 'kt':
    case 'kts':
      return 'kotlin'
    case 'swift':
      return 'swift'
    case 'cpp':
    case 'cc':
    case 'cxx':
    case 'hpp':
    case 'hxx':
    case 'hh':
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
    case 'pl':
    case 'pm':
      return 'perl'
    case 'r':
      return 'r'
    case 'jl':
      return 'julia'
    case 'json':
    case 'jsonc':
    case 'json5':
    case 'jsonl':
      return 'json'
    case 'md':
    case 'markdown':
    case 'mdx':
      return 'markdown'
    case 'rst':
      return 'plaintext'
    case 'tex':
    case 'latex':
      return 'plaintext'
    case 'html':
    case 'htm':
    case 'xhtml':
      return 'html'
    case 'css':
      return 'css'
    case 'scss':
    case 'sass':
      return 'scss'
    case 'less':
      return 'less'
    case 'yaml':
    case 'yml':
      return 'yaml'
    case 'toml':
      return 'ini'
    case 'ini':
    case 'cfg':
    case 'conf':
    case 'properties':
      return 'ini'
    case 'xml':
    case 'xsd':
    case 'xsl':
    case 'xslt':
    case 'wxs':
    case 'csproj':
    case 'vbproj':
    case 'fsproj':
    case 'plist':
    case 'resx':
      return 'xml'
    case 'vue':
      return 'html'
    case 'svelte':
      return 'html'
    case 'astro':
      return 'html'
    case 'svg':
      return 'xml'
    case 'sh':
    case 'bash':
    case 'zsh':
    case 'fish':
    case 'ksh':
      return 'shell'
    case 'ps1':
    case 'psm1':
    case 'psd1':
      return 'powershell'
    case 'bat':
    case 'cmd':
      return 'bat'
    case 'sql':
      return 'sql'
    case 'graphql':
    case 'gql':
      return 'graphql'
    case 'proto':
      return 'plaintext'
    case 'dockerfile':
      return 'dockerfile'
    case 'lua':
      return 'lua'
    case 'dart':
      return 'dart'
    case 'scala':
    case 'sbt':
    case 'sc':
      return 'scala'
    case 'groovy':
    case 'gradle':
      return 'plaintext'
    case 'diff':
    case 'patch':
      return 'plaintext'
    case 'env':
    case 'lock':
    case 'log':
    case 'csv':
    case 'tsv':
    case 'txt':
      return 'plaintext'
    case 'ex':
    case 'exs':
      return 'plaintext'
    case 'hs':
      return 'plaintext'
    case 'clj':
    case 'cljs':
    case 'edn':
      return 'plaintext'
    case 'zig':
      return 'plaintext'
    case 'nim':
      return 'plaintext'
    case 'vim':
      return 'plaintext'
    case 'hcl':
    case 'tf':
    case 'tfvars':
      return 'plaintext'
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

function lspRangeToMonaco(range: {
  start: LspPosition
  end: LspPosition
}): MonacoNs.IRange {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  }
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

function flattenHover(result: unknown): {
  text: string | null
  range: { start: LspPosition; end: LspPosition } | null
} {
  if (!result || typeof result !== 'object') return { text: null, range: null }
  const r = result as { contents?: unknown; range?: unknown }
  const contents = r.contents
  if (!contents) return { text: null, range: null }
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
  let range: { start: LspPosition; end: LspPosition } | null = null
  const rawRange = r.range as
    | { start?: LspPosition; end?: LspPosition }
    | undefined
  if (
    rawRange?.start &&
    rawRange?.end &&
    typeof rawRange.start.line === 'number' &&
    typeof rawRange.end.line === 'number'
  ) {
    range = { start: rawRange.start, end: rawRange.end }
  }
  return { text: text.length > 0 ? text : null, range }
}

type LspLocation = {
  uri: string
  range: { start: LspPosition; end: LspPosition }
}

type LspLocationLink = {
  targetUri: string
  targetRange: { start: LspPosition; end: LspPosition }
  targetSelectionRange?: { start: LspPosition; end: LspPosition }
}

function flattenLocations(result: unknown): LspLocation[] {
  if (!result) return []
  const items = Array.isArray(result) ? result : [result]
  const out: LspLocation[] = []
  for (const item of items) {
    if (!item || typeof item !== 'object') continue
    const loc = item as LspLocation & LspLocationLink
    if (typeof loc.uri === 'string' && loc.range?.start && loc.range?.end) {
      out.push({ uri: loc.uri, range: loc.range })
      continue
    }
    if (typeof loc.targetUri === 'string' && loc.targetRange?.start && loc.targetRange?.end) {
      out.push({
        uri: loc.targetUri,
        range: loc.targetSelectionRange ?? loc.targetRange,
      })
    }
  }
  return out
}

type LspSymbolInformation = {
  name: string
  kind: number
  containerName?: string
  location?: LspLocation
  range?: { start: LspPosition; end: LspPosition }
  selectionRange?: { start: LspPosition; end: LspPosition }
  children?: LspSymbolInformation[]
  detail?: string
  tags?: number[]
}

const SYMBOL_KIND_TO_MONACO: Record<number, number> = {
  1: 4,
  2: 1,
  3: 2,
  4: 3,
  5: 4,
  6: 5,
  7: 6,
  8: 7,
  9: 8,
  10: 9,
  11: 10,
  12: 11,
  13: 12,
  14: 13,
  15: 14,
  16: 15,
  17: 16,
  18: 17,
  19: 18,
  20: 19,
  21: 20,
  22: 21,
  23: 22,
  24: 23,
  25: 24,
  26: 25,
}

type LspInlayHint = {
  position: LspPosition
  label: string | Array<{ value: string }>
  kind?: number
  paddingLeft?: boolean
  paddingRight?: boolean
  tooltip?: string | { value: string }
}

function inlayHintLabel(hint: LspInlayHint): string {
  if (typeof hint.label === 'string') return hint.label
  if (Array.isArray(hint.label)) {
    return hint.label.map((part) => part.value).join('')
  }
  return ''
}

type LspParameterInformation = {
  label: string | [number, number]
  documentation?: string | { value: string }
}

type LspSignatureInformation = {
  label: string
  documentation?: string | { value: string }
  parameters?: LspParameterInformation[]
  activeParameter?: number
}

type LspSignatureHelp = {
  signatures: LspSignatureInformation[]
  activeSignature?: number
  activeParameter?: number
}

type LspTextEdit = {
  range: { start: LspPosition; end: LspPosition }
  newText: string
}

type LspCommand = {
  title: string
  command: string
  arguments?: unknown[]
}

type LspWorkspaceEdit = {
  changes?: Record<string, LspTextEdit[]>
  documentChanges?: Array<
    | {
        textDocument: { uri: string; version?: number | null }
        edits: LspTextEdit[]
      }
    | {
        kind: 'create' | 'rename' | 'delete'
        uri?: string
        oldUri?: string
        newUri?: string
      }
  >
}

type LspCodeAction = {
  title: string
  kind?: string
  isPreferred?: boolean
  disabled?: { reason?: string }
  diagnostics?: LspDiagnostic[]
  edit?: LspWorkspaceEdit
  command?: LspCommand
}

function lspMarkerToDiagnostic(
  monaco: typeof MonacoNs,
  marker: MonacoNs.editor.IMarkerData,
): Record<string, unknown> {
  const severity = (() => {
    switch (marker.severity) {
      case monaco.MarkerSeverity.Error:
        return 1
      case monaco.MarkerSeverity.Warning:
        return 2
      case monaco.MarkerSeverity.Info:
        return 3
      case monaco.MarkerSeverity.Hint:
        return 4
      default:
        return 3
    }
  })()
  const diag: Record<string, unknown> = {
    range: {
      start: { line: marker.startLineNumber - 1, character: marker.startColumn - 1 },
      end: { line: marker.endLineNumber - 1, character: marker.endColumn - 1 },
    },
    severity,
    message: marker.message ?? '',
  }
  if (marker.source) diag.source = marker.source
  if (marker.code != null) {
    diag.code = typeof marker.code === 'string' ? marker.code : String(marker.code)
  }
  return diag
}

function flattenLspCodeActions(result: unknown): LspCodeAction[] {
  if (!Array.isArray(result)) return []
  const out: LspCodeAction[] = []
  for (const raw of result) {
    if (!raw || typeof raw !== 'object') continue
    const item = raw as LspCodeAction & LspCommand
    if (typeof (item as LspCommand).command === 'string' && typeof item.title === 'string') {
      out.push({
        title: item.title,
        command: {
          title: item.title,
          command: (item as LspCommand).command,
          arguments: (item as LspCommand).arguments,
        },
      })
      continue
    }
    if (typeof item.title === 'string') {
      out.push(item)
    }
  }
  return out
}

function buildLineOffsets(text: string): number[] {
  const offsets = [0]
  for (let i = 0; i < text.length; i += 1) {
    if (text.charCodeAt(i) === 10) {
      offsets.push(i + 1)
    }
  }
  return offsets
}

function offsetFromPosition(offsets: number[], pos: LspPosition): number {
  const line = Math.max(0, Math.min(pos.line, offsets.length - 1))
  const base = offsets[line] ?? 0
  return base + Math.max(0, pos.character)
}

function applyTextEditsToString(text: string, edits: LspTextEdit[]): string {
  const sane = edits.filter((e) => e?.range?.start && e?.range?.end)
  if (sane.length === 0) return text
  const sorted = [...sane].sort((a, b) => {
    if (a.range.start.line !== b.range.start.line) {
      return b.range.start.line - a.range.start.line
    }
    if (a.range.start.character !== b.range.start.character) {
      return b.range.start.character - a.range.start.character
    }
    if (a.range.end.line !== b.range.end.line) {
      return b.range.end.line - a.range.end.line
    }
    return b.range.end.character - a.range.end.character
  })
  const offsets = buildLineOffsets(text)
  let out = text
  for (const edit of sorted) {
    const startOff = offsetFromPosition(offsets, edit.range.start)
    const endOff = offsetFromPosition(offsets, edit.range.end)
    const lo = Math.min(startOff, endOff)
    const hi = Math.max(startOff, endOff)
    out = out.slice(0, lo) + (edit.newText ?? '') + out.slice(hi)
  }
  return out
}

function collectWorkspaceEditsByUri(edit: LspWorkspaceEdit): Map<string, LspTextEdit[]> {
  const map = new Map<string, LspTextEdit[]>()
  if (edit.changes) {
    for (const [uri, edits] of Object.entries(edit.changes)) {
      if (!Array.isArray(edits)) continue
      const sane = edits.filter((e) => e?.range?.start && e?.range?.end)
      if (sane.length === 0) continue
      const existing = map.get(uri) ?? []
      map.set(uri, existing.concat(sane))
    }
  }
  if (Array.isArray(edit.documentChanges)) {
    for (const change of edit.documentChanges) {
      if (!change || typeof change !== 'object') continue
      const doc = change as {
        textDocument?: { uri: string }
        edits?: LspTextEdit[]
      }
      if (doc.textDocument?.uri && Array.isArray(doc.edits)) {
        const sane = doc.edits.filter((e) => e?.range?.start && e?.range?.end)
        if (sane.length === 0) continue
        const existing = map.get(doc.textDocument.uri) ?? []
        map.set(doc.textDocument.uri, existing.concat(sane))
      }
    }
  }
  return map
}

function lookupRegisteredModel(
  monaco: typeof MonacoNs | null,
  relPath: string,
): MonacoModelHandle | null {
  if (!relPath) return null
  const store = useWorkspaceFilesStore.getState()
  const registered = store.monacoModels[relPath]
  if (registered && !registered.isDisposed?.()) return registered
  if (!monaco) return null
  try {
    const uri = monaco.Uri.parse(relPath)
    const model = monaco.editor.getModel(uri)
    if (model && !model.isDisposed()) {
      const handle = model as unknown as MonacoModelHandle
      store.registerMonacoModel(relPath, handle)
      return handle
    }
  } catch {}
  return null
}

function uriToRelPath(uri: string, workspaceRoot: string): string | null {
  if (!uri.startsWith('file://')) return null
  let path = decodeURIComponent(uri.slice('file://'.length))
  if (path.startsWith('/') && /^\/[A-Za-z]:/.test(path)) {
    path = path.slice(1)
  }
  const normalizedRoot = workspaceRoot.replace(/\\/g, '/').replace(/\/$/, '')
  const normalizedPath = path.replace(/\\/g, '/')
  if (
    normalizedPath === normalizedRoot ||
    normalizedPath.toLowerCase() === normalizedRoot.toLowerCase()
  ) {
    return ''
  }
  const prefix = `${normalizedRoot}/`
  if (normalizedPath.startsWith(prefix)) {
    return normalizedPath.slice(prefix.length)
  }
  if (normalizedPath.toLowerCase().startsWith(prefix.toLowerCase())) {
    return normalizedPath.slice(prefix.length)
  }
  return null
}

type LspCompletionItem = {
  label: string | { label: string; detail?: string; description?: string }
  kind?: number
  detail?: string
  documentation?: string | { kind?: string; value?: string }
  insertText?: string
  insertTextFormat?: number
  insertTextMode?: number
  filterText?: string
  sortText?: string
  preselect?: boolean
  deprecated?: boolean
  tags?: number[]
  command?: { title: string; command: string; arguments?: unknown[] }
  data?: unknown
  textEdit?: {
    newText?: string
    range?: { start: LspPosition; end: LspPosition }
    insert?: { start: LspPosition; end: LspPosition }
    replace?: { start: LspPosition; end: LspPosition }
  }
  additionalTextEdits?: LspTextEdit[]
}

type LspCompletionList = {
  isIncomplete?: boolean
  items: LspCompletionItem[]
}

function flattenCompletions(result: unknown): {
  items: LspCompletionItem[]
  isIncomplete: boolean
} {
  if (!result) return { items: [], isIncomplete: false }
  if (Array.isArray(result)) {
    return { items: result as LspCompletionItem[], isIncomplete: false }
  }
  const r = result as LspCompletionList
  if (Array.isArray(r.items)) {
    return { items: r.items, isIncomplete: !!r.isIncomplete }
  }
  return { items: [], isIncomplete: false }
}

function completionLabelString(label: LspCompletionItem['label']): string {
  if (typeof label === 'string') return label
  if (label && typeof label === 'object' && typeof label.label === 'string') {
    return label.label
  }
  return ''
}

function completionLabelDetails(label: LspCompletionItem['label']):
  | { detail?: string; description?: string }
  | undefined {
  if (label && typeof label === 'object' && (label.detail || label.description)) {
    return { detail: label.detail, description: label.description }
  }
  return undefined
}

function docToMarkdown(
  doc: LspCompletionItem['documentation'],
): MonacoNs.IMarkdownString | string | undefined {
  if (!doc) return undefined
  if (typeof doc === 'string') return doc
  if (typeof doc === 'object' && typeof doc.value === 'string') {
    if (doc.kind === 'markdown') {
      return { value: doc.value, isTrusted: false, supportThemeIcons: true }
    }
    return doc.value
  }
  return undefined
}

function lspTextEditToMonacoEdit(
  edit: LspTextEdit | null | undefined,
): MonacoNs.languages.TextEdit | null {
  if (!edit?.range?.start || !edit?.range?.end) return null
  return {
    range: lspRangeToMonaco(edit.range),
    text: edit.newText ?? '',
  }
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
  const pendingNavigation = useWorkspaceFilesStore((s) => s.pendingNavigation)
  const consumeNavigation = useWorkspaceFilesStore((s) => s.consumeNavigation)
  const inlayHintsEnabled = useLspStore((s) => s.preferences.inlayHintsEnabled)
  const formatOnSave = useLspStore((s) => s.preferences.formatOnSave)
  const hoverDelayMs = useLspStore((s) => s.preferences.hoverDelayMs)
  const preferencesLoaded = useLspStore((s) => s.preferencesLoaded)
  const fetchPreferences = useLspStore((s) => s.fetchPreferences)
  useEffect(() => {
    if (!preferencesLoaded) {
      void fetchPreferences()
    }
  }, [fetchPreferences, preferencesLoaded])
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

  const absPath = useMemo(() => {
    if (!root || !activeTab) return null
    return joinWorkspaceAbsPath(root, activeTab)
  }, [root, activeTab])

  const fileUri = useMemo(() => {
    if (!absPath) return null
    return workspaceAbsPathToUri(absPath)
  }, [absPath])

  const languageId = useMemo(() => {
    if (!activeTab) return 'plaintext'
    return languageIdFor(nameOf(activeTab))
  }, [activeTab])

  const fileName = useMemo(() => (activeTab ? nameOf(activeTab) : ''), [activeTab])
  const isMarkdown = useMemo(() => isMarkdownExt(fileName), [fileName])

  const [editorPrefs, setEditorPrefs] = useState<EditorPrefs>(() => readEditorPrefs())
  const togglePref = useCallback((key: keyof EditorPrefs) => {
    setEditorPrefs((prev) => {
      const next = { ...prev, [key]: !prev[key] }
      writeEditorPrefs(next)
      return next
    })
  }, [])

  const [markdownView, setMarkdownView] = useState<'source' | 'preview'>('preview')
  useEffect(() => {
    setMarkdownView(isMarkdown ? 'preview' : 'source')
  }, [activeTab, isMarkdown])

  const [largeFileAck, setLargeFileAck] = useState<{
    relPath: string
    mode: 'open' | 'truncate'
  } | null>(null)
  useEffect(() => {
    setLargeFileAck(null)
  }, [activeTab])

  const handleCopyAbsolutePath = useCallback(async () => {
    if (!absPath) return
    const ok = await copyTextToClipboard(absPath)
    addToast({
      type: ok ? 'success' : 'error',
      message: ok ? t('files.preview.copied') : t('files.preview.copyFailed'),
    })
  }, [absPath, addToast, t])

  const handleCopyRelativePath = useCallback(async () => {
    if (!activeTab) return
    const ok = await copyTextToClipboard(activeTab)
    addToast({
      type: ok ? 'success' : 'error',
      message: ok ? t('files.preview.copied') : t('files.preview.copyFailed'),
    })
  }, [activeTab, addToast, t])

  const handleReveal = useCallback(async () => {
    if (!absPath) return
    try {
      await revealInExplorer(absPath)
    } catch (err) {
      addToast({
        type: 'error',
        message: t('files.preview.revealFailed', {
          message: err instanceof Error ? err.message : String(err),
        }),
      })
    }
  }, [absPath, addToast, t])

  const canReveal = useMemo(() => isTauriRuntime(), [])

  const draftText = !buffer || buffer.isBinary ? '' : buffer.draft ?? ''
  const totalLines = useMemo(() => countLines(draftText), [draftText])
  const draftSizeBytes = buffer?.sizeBytes ?? 0
  const isLargeFile =
    draftSizeBytes > LARGE_FILE_BYTE_THRESHOLD ||
    totalLines > LARGE_FILE_LINE_THRESHOLD
  const ackMatches = !!(
    largeFileAck && activeTab && largeFileAck.relPath === activeTab
  )
  const showLargeFileGuard = isLargeFile && !ackMatches
  const truncated = ackMatches && largeFileAck?.mode === 'truncate'
  const editorValue = useMemo(() => {
    if (!truncated) return draftText
    let cut = 0
    let lines = 0
    for (let i = 0; i < draftText.length; i += 1) {
      if (draftText.charCodeAt(i) === 10) {
        lines += 1
        if (lines >= LARGE_FILE_TRUNCATE_LINES) {
          cut = i + 1
          break
        }
      }
    }
    return cut > 0 ? draftText.slice(0, cut) : draftText
  }, [draftText, truncated])

  const editorRef = useRef<MonacoNs.editor.IStandaloneCodeEditor | null>(null)
  const monacoRef = useRef<typeof MonacoNs | null>(null)

  const ctxRef = useRef<{
    uri: string | null
    languageId: string
    workspaceRoot: string | null
    inlayHintsEnabled: boolean
    hoverDelayMs: number
  }>({
    uri: null,
    languageId: 'plaintext',
    workspaceRoot: null,
    inlayHintsEnabled: true,
    hoverDelayMs: 250,
  })
  ctxRef.current.uri = fileUri
  ctxRef.current.languageId = languageId
  ctxRef.current.workspaceRoot = root
  ctxRef.current.inlayHintsEnabled = inlayHintsEnabled
  ctxRef.current.hoverDelayMs = hoverDelayMs

  useEffect(() => {
    if (!fileUri || !buffer || buffer.isBinary) return
    const text = buffer.draft ?? ''
    void lspBridge.didOpen({ uri: fileUri, languageId, text })
    return () => {
      void lspBridge.didClose(fileUri)
    }    
  }, [fileUri, buffer?.isBinary])

  useEffect(() => {
    if (!fileUri || !buffer || buffer.isBinary) return
    lspBridge.didChange({
      uri: fileUri,
      languageId,
      text: buffer.draft ?? '',
    })    
  }, [fileUri, buffer?.draft, buffer?.isBinary])

  const runFormatBeforeSave = useCallback(async () => {
    const editor = editorRef.current
    const monaco = monacoRef.current
    if (!editor || !monaco) return
    const model = editor.getModel()
    if (!model) return
    const uri = ctxRef.current.uri
    if (!uri) return
    try {
      const result = await lspBridge.formatting({
        uri,
        languageId: ctxRef.current.languageId,
        text: model.getValue(),
        options: {
          tabSize: model.getOptions().tabSize,
          insertSpaces: model.getOptions().insertSpaces,
        },
      })
      if (!Array.isArray(result) || result.length === 0) return
      const edits: MonacoNs.editor.IIdentifiedSingleEditOperation[] = []
      for (const edit of result as LspTextEdit[]) {
        if (!edit?.range?.start || !edit?.range?.end) continue
        edits.push({
          range: {
            startLineNumber: edit.range.start.line + 1,
            startColumn: edit.range.start.character + 1,
            endLineNumber: edit.range.end.line + 1,
            endColumn: edit.range.end.character + 1,
          },
          text: edit.newText,
          forceMoveMarkers: true,
        })
      }
      if (edits.length > 0) {
        editor.executeEdits('format-on-save', edits)
      }
    } catch (err) {
      console.warn('[lsp] formatting failed', err)
    }
  }, [])

  const handleSave = useCallback(async () => {
    if (!activeTab || !buffer || buffer.isBinary) return
    if (formatOnSave) {
      await runFormatBeforeSave()
      if (fileUri) {
        const latest =
          useWorkspaceFilesStore.getState().files[`${root}::${activeTab}`]
        await lspBridge.willSave({
          uri: fileUri,
          languageId,
          text: latest?.draft ?? buffer.draft ?? '',
        })
      }
    }
    const latestBuf =
      useWorkspaceFilesStore.getState().files[`${root}::${activeTab}`] ?? buffer
    if (!latestBuf.isDirty) return
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
  }, [
    activeTab,
    addToast,
    buffer,
    fileUri,
    formatOnSave,
    languageId,
    root,
    runFormatBeforeSave,
    saveFile,
    t,
  ])

  const currentDiagnostics = useLspStore((s) => {
    if (!fileUri) return undefined
    if (root && root.length > 0) {
      const bucket = s.diagnosticsByWorkspace[root]
      return bucket?.[fileUri]?.diagnostics
    }
    return s.diagnosticsByUri[fileUri]?.diagnostics
  })
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

  useEffect(() => {
    const editor = editorRef.current
    if (!editor) return
    editor.updateOptions({ hover: { enabled: true, delay: hoverDelayMs } })
  }, [hoverDelayMs])

  const providersRegistered = useRef<Set<string>>(new Set())
  const applyCodeActionCmdId = useRef<string | null>(null)

  const applyLspCodeAction = useCallback(
    async (action: LspCodeAction) => {
      const monaco = monacoRef.current
      const editor = editorRef.current
      if (!monaco || !editor) return
      const workspaceRoot = ctxRef.current.workspaceRoot
      const activeRel = useWorkspaceFilesStore.getState().activeTab
      if (!workspaceRoot) return

      let appliedFiles = 0

      if (action.edit) {
        const editsByUri = collectWorkspaceEditsByUri(action.edit)
        if (editsByUri.size > 0) {
          const originalActiveTab = useWorkspaceFilesStore.getState().activeTab
          const fallbackList: string[] = []
          let skippedUndo = 0
          for (const [uri, edits] of editsByUri.entries()) {
            const rel = uriToRelPath(uri, workspaceRoot)
            if (rel === null) {
              fallbackList.push(uri)
              continue
            }
            const buildMonacoEdits = (): MonacoEditOperation[] => {
              const out: MonacoEditOperation[] = []
              for (const e of edits) {
                out.push({
                  range: {
                    startLineNumber: e.range.start.line + 1,
                    startColumn: e.range.start.character + 1,
                    endLineNumber: e.range.end.line + 1,
                    endColumn: e.range.end.character + 1,
                  },
                  text: e.newText,
                  forceMoveMarkers: true,
                })
              }
              return out
            }
            if (rel === activeRel) {
              const monacoEdits = buildMonacoEdits()
              if (monacoEdits.length > 0) {
                editor.executeEdits(
                  'lsp-code-action',
                  monacoEdits as MonacoNs.editor.IIdentifiedSingleEditOperation[],
                )
                appliedFiles += 1
              }
              continue
            }
            const registered = lookupRegisteredModel(monaco, rel)
            if (registered && !registered.isDisposed?.()) {
              const monacoEdits = buildMonacoEdits()
              if (monacoEdits.length > 0) {
                try {
                  registered.pushEditOperations(null, monacoEdits, () => null)
                  const fullModel = registered as MonacoNs.editor.ITextModel
                  useWorkspaceFilesStore
                    .getState()
                    .updateDraft(rel, fullModel.getValue())
                  appliedFiles += 1
                } catch {
                  fallbackList.push(rel)
                }
              }
              continue
            }
            try {
              await useWorkspaceFilesStore.getState().openTab(rel)
              for (let i = 0; i < 30; i += 1) {
                const buf =
                  useWorkspaceFilesStore.getState().files[`${workspaceRoot}::${rel}`]
                if (buf && !buf.loading) break
                await new Promise((r) => setTimeout(r, 50))
              }
              const buf =
                useWorkspaceFilesStore.getState().files[`${workspaceRoot}::${rel}`]
              if (!buf || buf.isBinary) {
                fallbackList.push(rel)
                continue
              }
              const newContent = applyTextEditsToString(buf.draft, edits)
              if (newContent !== buf.draft) {
                useWorkspaceFilesStore.getState().updateDraft(rel, newContent)
                appliedFiles += 1
                skippedUndo += 1
              }
            } catch {
              fallbackList.push(rel)
            }
          }
          if (originalActiveTab && originalActiveTab !== activeRel) {
            useWorkspaceFilesStore.getState().setActiveTab(originalActiveTab)
          }
          if (fallbackList.length > 0) {
            addToast({
              type: 'error',
              message: t('lsp.codeAction.partialFailure', {
                files: fallbackList.join(', '),
              }),
            })
          }
          if (skippedUndo > 0) {
            addToast({
              type: 'info',
              message: t('lsp.codeAction.skippedUndo', { count: skippedUndo }),
            })
          }
        }
      }

      if (action.command) {
        try {
          await lspBridge.executeCommand({
            uri: ctxRef.current.uri ?? undefined,
            languageId: ctxRef.current.languageId,
            command: action.command.command,
            arguments: action.command.arguments,
          })
        } catch (err) {
          addToast({
            type: 'error',
            message: t('lsp.codeAction.commandFailed', {
              message: err instanceof Error ? err.message : String(err),
            }),
          })
          return
        }
      }

      if (appliedFiles > 0) {
        addToast({
          type: 'success',
          message: t('lsp.codeAction.applied', { count: appliedFiles }),
        })
      }
    },
    [addToast, t],
  )

  const applyLspCodeActionRef = useRef(applyLspCodeAction)
  applyLspCodeActionRef.current = applyLspCodeAction

  useEffect(() => {
    if (!activeTab) {
      useUIStore.getState().setEditorCursor(null)
    }
    return () => {
      const current = useUIStore.getState().editorCursor
      if (current && (!activeTab || current.relPath === activeTab)) {
        useUIStore.getState().setEditorCursor(null)
      }
    }
  }, [activeTab])

  const onMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor
    monacoRef.current = monaco

    const currentActiveTab = (): string | null =>
      useWorkspaceFilesStore.getState().activeTab

    editor.updateOptions({ hover: { enabled: true, delay: ctxRef.current.hoverDelayMs } })

    const registerCurrentModel = () => {
      const model = editor.getModel()
      const rel = useWorkspaceFilesStore.getState().activeTab
      if (!model || !rel) return
      const handle = model as unknown as MonacoModelHandle
      useWorkspaceFilesStore.getState().registerMonacoModel(rel, handle)
    }
    registerCurrentModel()

    const cmdId = editor.addCommand(
      0,
      (_accessor: unknown, ...args: unknown[]) => {
        const raw = args[0]
        if (!raw || typeof raw !== 'object') return
        void applyLspCodeActionRef.current(raw as LspCodeAction)
      },
    )
    applyCodeActionCmdId.current = cmdId ?? null

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void handleSave()
    })

    editor.addAction({
      id: 'sen.copyAsMarkdown',
      label: t('editor.action.copyAsMarkdown'),
      contextMenuGroupId: '9_cutcopypaste',
      contextMenuOrder: 4,
      run: async (ed) => {
        const model = ed.getModel()
        if (!model) return
        const selection = ed.getSelection()
        if (!selection) return
        let startLine = selection.startLineNumber
        let endLine = selection.endLineNumber
        let text = model.getValueInRange(selection)
        if (!text) {
          text = model.getValue()
          startLine = 1
          endLine = model.getLineCount()
        }
        const lang = model.getLanguageId() || ''
        const rel = currentActiveTab() ?? ''
        const header =
          startLine === endLine
            ? `${rel}:${startLine}`
            : `${rel}:${startLine}-${endLine}`
        const snippet = `\`\`\`${lang} ${header}\n${text}\n\`\`\``
        const ok = await copyTextToClipboard(snippet)
        addToast({
          type: ok ? 'success' : 'error',
          message: ok
            ? t('files.tree.copyMarkdownDone')
            : t('files.tree.copyMarkdownFailed'),
        })
      },
    })

    const openLocationsInTabs = async (locations: LspLocation[]) => {
      if (locations.length === 0) return
      const workspaceRoot = ctxRef.current.workspaceRoot
      if (!workspaceRoot) return
      const first = locations[0]
      if (!first) return
      const rel = uriToRelPath(first.uri, workspaceRoot)
      if (rel === null) return
      if (rel === currentActiveTab()) {
        const range: MonacoNs.IRange = {
          startLineNumber: first.range.start.line + 1,
          startColumn: first.range.start.character + 1,
          endLineNumber: first.range.end.line + 1,
          endColumn: first.range.end.character + 1,
        }
        editor.revealRangeInCenter(range)
        editor.setSelection(range)
        return
      }
      try {
        await useWorkspaceFilesStore.getState().openTab(rel)
      } catch {
        return
      }
      const tick = () => {
        const next = editorRef.current
        if (!next) return
        const model = next.getModel()
        if (!model) {
          setTimeout(tick, 50)
          return
        }
        const range: MonacoNs.IRange = {
          startLineNumber: first.range.start.line + 1,
          startColumn: first.range.start.character + 1,
          endLineNumber: first.range.end.line + 1,
          endColumn: first.range.end.character + 1,
        }
        next.revealRangeInCenter(range)
        next.setSelection(range)
      }
      setTimeout(tick, 50)
    }

    const tokenToSignal = (
      token?: MonacoNs.CancellationToken | undefined,
    ): AbortSignal | undefined => {
      if (!token) return undefined
      const controller = new AbortController()
      if (token.isCancellationRequested) {
        controller.abort()
      } else {
        token.onCancellationRequested(() => controller.abort())
      }
      return controller.signal
    }

    const ensureProviders = (lang: string) => {
      if (providersRegistered.current.has(lang)) return
      providersRegistered.current.add(lang)

      monaco.languages.registerHoverProvider(lang, {
        provideHover: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
          token?: MonacoNs.CancellationToken,
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
              signal: tokenToSignal(token),
            })
            const { text, range } = flattenHover(result)
            if (!text) return null
            return {
              contents: [{ value: text, isTrusted: false, supportThemeIcons: true }],
              range: range ? lspRangeToMonaco(range) : undefined,
            }
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerCompletionItemProvider(lang, {
        triggerCharacters: ['.', ':', '/', '@', '<', '"', "'", '#', '$'],
        provideCompletionItems: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
          context: MonacoNs.languages.CompletionContext,
          token?: MonacoNs.CancellationToken,
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
              triggerKind: context.triggerKind + 1,
              triggerCharacter: context.triggerCharacter,
              signal: tokenToSignal(token),
            })
            const { items, isIncomplete } = flattenCompletions(result)
            const word = model.getWordUntilPosition(position)
            const fallbackRange: MonacoNs.IRange = {
              startLineNumber: position.lineNumber,
              endLineNumber: position.lineNumber,
              startColumn: word.startColumn,
              endColumn: word.endColumn,
            }
            return {
              incomplete: isIncomplete,
              suggestions: items.map((item) => {
                const labelText = completionLabelString(item.label)
                const labelDetails = completionLabelDetails(item.label)
                const newText =
                  item.textEdit?.newText ?? item.insertText ?? labelText
                const isSnippet = item.insertTextFormat === 2
                let range: MonacoNs.IRange | {
                  insert: MonacoNs.IRange
                  replace: MonacoNs.IRange
                } = fallbackRange
                if (item.textEdit) {
                  if (item.textEdit.insert && item.textEdit.replace) {
                    range = {
                      insert: lspRangeToMonaco(item.textEdit.insert),
                      replace: lspRangeToMonaco(item.textEdit.replace),
                    }
                  } else if (item.textEdit.range) {
                    range = lspRangeToMonaco(item.textEdit.range)
                  }
                }
                const additionalEdits = (item.additionalTextEdits ?? [])
                  .map((e) => lspTextEditToMonacoEdit(e))
                  .filter((e): e is MonacoNs.languages.TextEdit => e !== null)
                const suggestion: MonacoNs.languages.CompletionItem = {
                  label: labelDetails
                    ? { label: labelText, detail: labelDetails.detail, description: labelDetails.description }
                    : labelText,
                  kind:
                    (item.kind != null
                      ? COMPLETION_KIND_TO_MONACO_KIND[item.kind]
                      : monaco.languages.CompletionItemKind.Text) ??
                    monaco.languages.CompletionItemKind.Text,
                  insertText: newText,
                  insertTextRules: isSnippet
                    ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
                    : monaco.languages.CompletionItemInsertTextRule.None,
                  detail: item.detail,
                  documentation: docToMarkdown(item.documentation),
                  sortText: item.sortText,
                  filterText: item.filterText,
                  preselect: item.preselect,
                  tags: Array.isArray(item.tags) ? (item.tags as MonacoNs.languages.CompletionItemTag[]) : undefined,
                  range,
                  additionalTextEdits: additionalEdits.length > 0 ? additionalEdits : undefined,
                  command: item.command
                    ? {
                        id: item.command.command,
                        title: item.command.title,
                        arguments: item.command.arguments,
                      }
                    : undefined,
                }
                ;(suggestion as unknown as { _lsp: LspCompletionItem })._lsp = item
                return suggestion
              }),
            }
          } catch {
            return { suggestions: [] }
          }
        },
        resolveCompletionItem: async (item: MonacoNs.languages.CompletionItem) => {
          const original = (item as unknown as { _lsp?: LspCompletionItem })._lsp
          if (!original) return item
          try {
            const resolved = (await lspBridge.completionResolve({
              item: original,
              uri: ctxRef.current.uri ?? undefined,
              languageId: ctxRef.current.languageId,
            })) as LspCompletionItem | null
            if (!resolved || typeof resolved !== 'object') return item
            const next = { ...item } as MonacoNs.languages.CompletionItem
            if (resolved.detail && !next.detail) next.detail = resolved.detail
            if (resolved.documentation) {
              const doc = docToMarkdown(resolved.documentation)
              if (doc) next.documentation = doc
            }
            if (Array.isArray(resolved.additionalTextEdits) && resolved.additionalTextEdits.length > 0) {
              const edits = resolved.additionalTextEdits
                .map((e) => lspTextEditToMonacoEdit(e))
                .filter((e): e is MonacoNs.languages.TextEdit => e !== null)
              if (edits.length > 0) next.additionalTextEdits = edits
            }
            if (resolved.command && !next.command) {
              next.command = {
                id: resolved.command.command,
                title: resolved.command.title,
                arguments: resolved.command.arguments,
              }
            }
            return next
          } catch {
            return item
          }
        },
      })

      monaco.languages.registerDefinitionProvider(lang, {
        provideDefinition: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
          token?: MonacoNs.CancellationToken,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const workspaceRoot = ctxRef.current.workspaceRoot
          if (!workspaceRoot) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.definition({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
              signal: tokenToSignal(token),
            })
            const locations = flattenLocations(result)
            if (locations.length === 0) return null
            const monacoLocations: MonacoNs.languages.Location[] = []
            for (const loc of locations) {
              const rel = uriToRelPath(loc.uri, workspaceRoot)
              if (rel === null) continue
              const targetUri =
                rel === currentActiveTab()
                  ? model.uri
                  : monaco.Uri.parse(loc.uri)
              monacoLocations.push({
                uri: targetUri,
                range: {
                  startLineNumber: loc.range.start.line + 1,
                  startColumn: loc.range.start.character + 1,
                  endLineNumber: loc.range.end.line + 1,
                  endColumn: loc.range.end.character + 1,
                },
              })
            }
            if (monacoLocations.length > 0) {
              const first = locations[0]
              if (first) {
                const rel = uriToRelPath(first.uri, workspaceRoot)
                if (rel !== null && rel !== currentActiveTab()) {
                  void openLocationsInTabs(locations)
                  return null
                }
              }
            }
            return monacoLocations
          } catch {
            return null
          }
        },
      })

      if (monaco.languages.registerDeclarationProvider) {
        monaco.languages.registerDeclarationProvider(lang, {
          provideDeclaration: async (
            model: MonacoNs.editor.ITextModel,
            position: MonacoNs.Position,
            token?: MonacoNs.CancellationToken,
          ) => {
            const uri = ctxRef.current.uri
            if (!uri) return null
            const workspaceRoot = ctxRef.current.workspaceRoot
            if (!workspaceRoot) return null
            const lspPos = monacoPosToLsp(position)
            try {
              const result = await lspBridge.declaration({
                uri,
                languageId: ctxRef.current.languageId,
                line: lspPos.line,
                character: lspPos.character,
                text: model.getValue(),
                signal: tokenToSignal(token),
              })
              const locations = flattenLocations(result)
              if (locations.length === 0) return null
              const out: MonacoNs.languages.Location[] = []
              for (const loc of locations) {
                const rel = uriToRelPath(loc.uri, workspaceRoot)
                if (rel === null) continue
                out.push({
                  uri:
                    rel === currentActiveTab()
                      ? model.uri
                      : monaco.Uri.parse(loc.uri),
                  range: lspRangeToMonaco(loc.range),
                })
              }
              return out
            } catch {
              return null
            }
          },
        })
      }

      monaco.languages.registerLinkProvider(lang, {
        provideLinks: async (
          model: MonacoNs.editor.ITextModel,
          token?: MonacoNs.CancellationToken,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return { links: [] }
          try {
            const result = (await lspBridge.documentLink({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
              signal: tokenToSignal(token),
            })) as Array<{
              range: {
                start: { line: number; character: number }
                end: { line: number; character: number }
              }
              target?: string
              tooltip?: string
            }> | null
            if (!Array.isArray(result)) return { links: [] }
            const links: MonacoNs.languages.ILink[] = []
            for (const link of result) {
              if (!link?.range?.start || !link?.range?.end) continue
              const url = typeof link.target === 'string' ? link.target : undefined
              links.push({
                range: lspRangeToMonaco(link.range),
                url,
                tooltip: link.tooltip,
              })
            }
            return { links }
          } catch {
            return { links: [] }
          }
        },
      })

      monaco.languages.registerDocumentSemanticTokensProvider(lang, {
        getLegend: () => ({ tokenTypes: [], tokenModifiers: [] }),
        provideDocumentSemanticTokens: async (
          model: MonacoNs.editor.ITextModel,
          _lastResultId: string | null,
          token?: MonacoNs.CancellationToken,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          try {
            const result = (await lspBridge.semanticTokensFull({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
              signal: tokenToSignal(token),
            })) as { data?: number[]; resultId?: string } | null
            if (!result || !Array.isArray(result.data)) return null
            return {
              data: new Uint32Array(result.data),
              resultId: typeof result.resultId === 'string' ? result.resultId : undefined,
            }
          } catch {
            return null
          }
        },
        releaseDocumentSemanticTokens: () => undefined,
      })

      monaco.languages.registerReferenceProvider(lang, {
        provideReferences: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
          _context: MonacoNs.languages.ReferenceContext,
          token?: MonacoNs.CancellationToken,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const workspaceRoot = ctxRef.current.workspaceRoot
          if (!workspaceRoot) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.references({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
              signal: tokenToSignal(token),
            })
            const locations = flattenLocations(result)
            if (locations.length === 0) return null
            const out: MonacoNs.languages.Location[] = []
            for (const loc of locations) {
              const rel = uriToRelPath(loc.uri, workspaceRoot)
              if (rel === null) continue
              out.push({
                uri:
                  rel === currentActiveTab()
                    ? model.uri
                    : monaco.Uri.parse(loc.uri),
                range: {
                  startLineNumber: loc.range.start.line + 1,
                  startColumn: loc.range.start.character + 1,
                  endLineNumber: loc.range.end.line + 1,
                  endColumn: loc.range.end.character + 1,
                },
              })
            }
            return out
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerInlayHintsProvider(lang, {
        provideInlayHints: async (
          model: MonacoNs.editor.ITextModel,
          range: MonacoNs.Range,
        ) => {
          if (!ctxRef.current.inlayHintsEnabled) {
            return { hints: [], dispose: () => undefined }
          }
          const uri = ctxRef.current.uri
          if (!uri) return { hints: [], dispose: () => undefined }
          try {
            const result = await lspBridge.inlayHint({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
              range: {
                start: {
                  line: range.startLineNumber - 1,
                  character: range.startColumn - 1,
                },
                end: {
                  line: range.endLineNumber - 1,
                  character: range.endColumn - 1,
                },
              },
            })
            if (!Array.isArray(result)) {
              return { hints: [], dispose: () => undefined }
            }
            const hints: MonacoNs.languages.InlayHint[] = []
            for (const hint of result as LspInlayHint[]) {
              if (!hint?.position) continue
              hints.push({
                label: inlayHintLabel(hint),
                position: lspPosToMonaco(hint.position),
                kind: hint.kind === 2 ? 2 : 1,
                paddingLeft: hint.paddingLeft,
                paddingRight: hint.paddingRight,
                tooltip:
                  typeof hint.tooltip === 'string'
                    ? hint.tooltip
                    : hint.tooltip?.value,
              })
            }
            return { hints, dispose: () => undefined }
          } catch {
            return { hints: [], dispose: () => undefined }
          }
        },
      })

      monaco.languages.registerSignatureHelpProvider(lang, {
        signatureHelpTriggerCharacters: ['(', ','],
        signatureHelpRetriggerCharacters: [')'],
        provideSignatureHelp: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.signatureHelp({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
            })
            if (!result || typeof result !== 'object') return null
            const help = result as LspSignatureHelp
            if (!Array.isArray(help.signatures) || help.signatures.length === 0) {
              return null
            }
            return {
              value: {
                activeParameter: help.activeParameter ?? 0,
                activeSignature: help.activeSignature ?? 0,
                signatures: help.signatures.map((sig) => ({
                  label: sig.label,
                  documentation:
                    typeof sig.documentation === 'string'
                      ? sig.documentation
                      : sig.documentation?.value,
                  parameters: (sig.parameters ?? []).map((param) => ({
                    label: param.label,
                    documentation:
                      typeof param.documentation === 'string'
                        ? param.documentation
                        : param.documentation?.value,
                  })),
                  activeParameter: sig.activeParameter,
                })),
              },
              dispose: () => undefined,
            }
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerDocumentSymbolProvider(lang, {
        provideDocumentSymbols: async (model: MonacoNs.editor.ITextModel) => {
          const uri = ctxRef.current.uri
          if (!uri) return []
          try {
            const result = await lspBridge.documentSymbol({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
            })
            if (!Array.isArray(result)) return []
            const toMonaco = (
              sym: LspSymbolInformation,
            ): MonacoNs.languages.DocumentSymbol | null => {
              const r = sym.range ?? sym.location?.range
              const sr = sym.selectionRange ?? r
              if (!r?.start || !r?.end || !sr?.start || !sr?.end) return null
              const kindMonaco =
                SYMBOL_KIND_TO_MONACO[sym.kind] ??
                monaco.languages.SymbolKind.Variable
              return {
                name: sym.name,
                detail: sym.detail ?? '',
                kind: kindMonaco,
                tags: [],
                range: {
                  startLineNumber: r.start.line + 1,
                  startColumn: r.start.character + 1,
                  endLineNumber: r.end.line + 1,
                  endColumn: r.end.character + 1,
                },
                selectionRange: {
                  startLineNumber: sr.start.line + 1,
                  startColumn: sr.start.character + 1,
                  endLineNumber: sr.end.line + 1,
                  endColumn: sr.end.character + 1,
                },
                children: Array.isArray(sym.children)
                  ? (sym.children
                      .map(toMonaco)
                      .filter(Boolean) as MonacoNs.languages.DocumentSymbol[])
                  : [],
              }
            }
            const symbols: MonacoNs.languages.DocumentSymbol[] = []
            for (const sym of result as LspSymbolInformation[]) {
              const converted = toMonaco(sym)
              if (converted) symbols.push(converted)
            }
            return symbols
          } catch {
            return []
          }
        },
      })

      monaco.languages.registerCodeActionProvider(lang, {
        provideCodeActions: async (
          model: MonacoNs.editor.ITextModel,
          range: MonacoNs.Range,
          context: MonacoNs.languages.CodeActionContext,
        ) => {
          const uri = ctxRef.current.uri
          const cmdId = applyCodeActionCmdId.current
          if (!uri || !cmdId) {
            return { actions: [], dispose: () => undefined }
          }
          const diagnostics = context.markers.map((marker) =>
            lspMarkerToDiagnostic(monaco, marker),
          )
          const only =
            typeof context.only === 'string' && context.only.length > 0
              ? [context.only]
              : undefined
          try {
            const result = await lspBridge.codeAction({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
              range: {
                start: {
                  line: range.startLineNumber - 1,
                  character: range.startColumn - 1,
                },
                end: {
                  line: range.endLineNumber - 1,
                  character: range.endColumn - 1,
                },
              },
              diagnostics,
              only,
            })
            const lspActions = flattenLspCodeActions(result)
            const actions: MonacoNs.languages.CodeAction[] = lspActions.map((action) => ({
              title: action.title,
              kind: action.kind,
              isPreferred: action.isPreferred,
              disabled: action.disabled?.reason,
              diagnostics: undefined,
              command: {
                id: cmdId,
                title: action.title,
                arguments: [action],
              },
            }))
            return { actions, dispose: () => undefined }
          } catch {
            return { actions: [], dispose: () => undefined }
          }
        },
      })

      monaco.languages.registerDocumentFormattingEditProvider(lang, {
        provideDocumentFormattingEdits: async (
          model: MonacoNs.editor.ITextModel,
          options: MonacoNs.languages.FormattingOptions,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          try {
            const result = await lspBridge.formatting({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
              options: {
                tabSize: options.tabSize,
                insertSpaces: options.insertSpaces,
              },
            })
            if (!Array.isArray(result)) return null
            const edits: MonacoNs.languages.TextEdit[] = []
            for (const edit of result as LspTextEdit[]) {
              if (!edit?.range?.start || !edit?.range?.end) continue
              edits.push({
                range: lspRangeToMonaco(edit.range),
                text: edit.newText,
              })
            }
            return edits
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerDocumentRangeFormattingEditProvider(lang, {
        provideDocumentRangeFormattingEdits: async (
          model: MonacoNs.editor.ITextModel,
          range: MonacoNs.Range,
          options: MonacoNs.languages.FormattingOptions,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          try {
            const result = await lspBridge.rangeFormatting({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
              range: {
                start: {
                  line: range.startLineNumber - 1,
                  character: range.startColumn - 1,
                },
                end: {
                  line: range.endLineNumber - 1,
                  character: range.endColumn - 1,
                },
              },
              options: {
                tabSize: options.tabSize,
                insertSpaces: options.insertSpaces,
              },
            })
            if (!Array.isArray(result)) return null
            const edits: MonacoNs.languages.TextEdit[] = []
            for (const edit of result as LspTextEdit[]) {
              if (!edit?.range?.start || !edit?.range?.end) continue
              edits.push({
                range: lspRangeToMonaco(edit.range),
                text: edit.newText,
              })
            }
            return edits
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerDocumentHighlightProvider(lang, {
        provideDocumentHighlights: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.documentHighlight({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
            })
            if (!Array.isArray(result)) return null
            const out: MonacoNs.languages.DocumentHighlight[] = []
            for (const raw of result as Array<{
              range?: { start: LspPosition; end: LspPosition }
              kind?: number
            }>) {
              if (!raw?.range?.start || !raw?.range?.end) continue
              out.push({
                range: lspRangeToMonaco(raw.range),
                kind:
                  raw.kind === 2
                    ? monaco.languages.DocumentHighlightKind.Read
                    : raw.kind === 3
                    ? monaco.languages.DocumentHighlightKind.Write
                    : monaco.languages.DocumentHighlightKind.Text,
              })
            }
            return out
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerTypeDefinitionProvider(lang, {
        provideTypeDefinition: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const workspaceRoot = ctxRef.current.workspaceRoot
          if (!workspaceRoot) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.typeDefinition({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
            })
            const locations = flattenLocations(result)
            if (locations.length === 0) return null
            const out: MonacoNs.languages.Location[] = []
            for (const loc of locations) {
              const rel = uriToRelPath(loc.uri, workspaceRoot)
              if (rel === null) continue
              out.push({
                uri:
                  rel === currentActiveTab()
                    ? model.uri
                    : monaco.Uri.parse(loc.uri),
                range: lspRangeToMonaco(loc.range),
              })
            }
            return out
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerImplementationProvider(lang, {
        provideImplementation: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const workspaceRoot = ctxRef.current.workspaceRoot
          if (!workspaceRoot) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.implementation({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
            })
            const locations = flattenLocations(result)
            if (locations.length === 0) return null
            const out: MonacoNs.languages.Location[] = []
            for (const loc of locations) {
              const rel = uriToRelPath(loc.uri, workspaceRoot)
              if (rel === null) continue
              out.push({
                uri:
                  rel === currentActiveTab()
                    ? model.uri
                    : monaco.Uri.parse(loc.uri),
                range: lspRangeToMonaco(loc.range),
              })
            }
            return out
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerRenameProvider(lang, {
        provideRenameEdits: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
          newName: string,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const workspaceRoot = ctxRef.current.workspaceRoot
          if (!workspaceRoot) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = (await lspBridge.rename({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              newName,
              text: model.getValue(),
            })) as LspWorkspaceEdit | null
            if (!result || typeof result !== 'object') return null
            const editsByUri = collectWorkspaceEditsByUri(result)
            if (editsByUri.size === 0) return { edits: [] }
            const edits: MonacoNs.languages.IWorkspaceTextEdit[] = []
            for (const [docUri, lspEdits] of editsByUri.entries()) {
              const rel = uriToRelPath(docUri, workspaceRoot)
              const targetUri =
                rel === currentActiveTab()
                  ? model.uri
                  : monaco.Uri.parse(docUri)
              for (const e of lspEdits) {
                if (!e?.range?.start || !e?.range?.end) continue
                edits.push({
                  resource: targetUri,
                  versionId: undefined,
                  textEdit: {
                    range: lspRangeToMonaco(e.range),
                    text: e.newText ?? '',
                  },
                })
              }
            }
            return { edits }
          } catch {
            return null
          }
        },
        resolveRenameLocation: async (
          model: MonacoNs.editor.ITextModel,
          position: MonacoNs.Position,
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          const lspPos = monacoPosToLsp(position)
          try {
            const result = await lspBridge.prepareRename({
              uri,
              languageId: ctxRef.current.languageId,
              line: lspPos.line,
              character: lspPos.character,
              text: model.getValue(),
            })
            if (!result) return null
            const word = model.getWordAtPosition(position)
            const fallbackText = word?.word ?? ''
            const fallbackRange: MonacoNs.IRange = word
              ? {
                  startLineNumber: position.lineNumber,
                  endLineNumber: position.lineNumber,
                  startColumn: word.startColumn,
                  endColumn: word.endColumn,
                }
              : {
                  startLineNumber: position.lineNumber,
                  endLineNumber: position.lineNumber,
                  startColumn: position.column,
                  endColumn: position.column,
                }
            const r = result as
              | { start: LspPosition; end: LspPosition }
              | { range: { start: LspPosition; end: LspPosition }; placeholder?: string }
              | { defaultBehavior: boolean }
            if (typeof r === 'object' && 'defaultBehavior' in r && r.defaultBehavior) {
              return { range: fallbackRange, text: fallbackText }
            }
            if (typeof r === 'object' && 'range' in r && r.range?.start && r.range?.end) {
              return {
                range: lspRangeToMonaco(r.range),
                text: r.placeholder ?? fallbackText,
              }
            }
            if (typeof r === 'object' && 'start' in r && 'end' in r) {
              return {
                range: lspRangeToMonaco(r as { start: LspPosition; end: LspPosition }),
                text: fallbackText,
              }
            }
            return { range: fallbackRange, text: fallbackText }
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerFoldingRangeProvider(lang, {
        provideFoldingRanges: async (model: MonacoNs.editor.ITextModel) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          try {
            const result = await lspBridge.foldingRange({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
            })
            if (!Array.isArray(result)) return null
            const out: MonacoNs.languages.FoldingRange[] = []
            for (const raw of result as Array<{
              startLine: number
              endLine: number
              kind?: string
              startCharacter?: number
              endCharacter?: number
            }>) {
              if (typeof raw.startLine !== 'number' || typeof raw.endLine !== 'number') continue
              out.push({
                start: raw.startLine + 1,
                end: raw.endLine + 1,
                kind:
                  raw.kind === 'comment'
                    ? monaco.languages.FoldingRangeKind.Comment
                    : raw.kind === 'imports'
                    ? monaco.languages.FoldingRangeKind.Imports
                    : raw.kind === 'region'
                    ? monaco.languages.FoldingRangeKind.Region
                    : undefined,
              })
            }
            return out
          } catch {
            return null
          }
        },
      })

      monaco.languages.registerSelectionRangeProvider(lang, {
        provideSelectionRanges: async (
          model: MonacoNs.editor.ITextModel,
          positions: MonacoNs.Position[],
        ) => {
          const uri = ctxRef.current.uri
          if (!uri) return null
          try {
            const result = await lspBridge.selectionRange({
              uri,
              languageId: ctxRef.current.languageId,
              text: model.getValue(),
              positions: positions.map((p) => ({
                line: p.lineNumber - 1,
                character: p.column - 1,
              })),
            })
            if (!Array.isArray(result)) return null
            const out: MonacoNs.languages.SelectionRange[][] = []
            for (const node of result as Array<{
              range: { start: LspPosition; end: LspPosition }
              parent?: unknown
            } | null>) {
              const ranges: MonacoNs.languages.SelectionRange[] = []
              let cursor: typeof node = node
              while (cursor && cursor.range?.start && cursor.range?.end) {
                ranges.push({ range: lspRangeToMonaco(cursor.range) })
                cursor = (cursor.parent as typeof node) ?? null
              }
              out.push(ranges)
            }
            return out
          } catch {
            return null
          }
        },
      })
    }

    ensureProviders(languageId)

    const saveViewState = (() => {
      let pending = false
      return (rel: string | null) => {
        if (!rel) return
        if (pending) return
        pending = true
        window.setTimeout(() => {
          pending = false
          const e = editorRef.current
          if (!e) return
          const sel = e.getSelection()
          const scrollTop = e.getScrollTop?.() ?? 0
          const scrollLeft = e.getScrollLeft?.() ?? 0
          useWorkspaceFilesStore.getState().setTabViewState(rel, {
            scrollTop,
            scrollLeft,
            selection: sel
              ? {
                  startLineNumber: sel.startLineNumber,
                  startColumn: sel.startColumn,
                  endLineNumber: sel.endLineNumber,
                  endColumn: sel.endColumn,
                }
              : null,
          })
        }, 250)
      }
    })()

    const pushEditorCursor = () => {
      const rel = currentActiveTab()
      if (!rel) {
        useUIStore.getState().setEditorCursor(null)
        return
      }
      const pos = editor.getPosition()
      const sel = editor.getSelection()
      const model = editor.getModel()
      if (!pos) return
      const selectedCharCount =
        sel && model && !sel.isEmpty() ? model.getValueInRange(sel).length : 0
      useUIStore.getState().setEditorCursor({
        relPath: rel,
        line: pos.lineNumber,
        column: pos.column,
        selection: sel
          ? {
              startLine: sel.startLineNumber,
              startColumn: sel.startColumn,
              endLine: sel.endLineNumber,
              endColumn: sel.endColumn,
            }
          : null,
        selectedCharCount,
      })
    }

    editor.onDidChangeCursorSelection(() => {
      saveViewState(useWorkspaceFilesStore.getState().activeTab)
      pushEditorCursor()
    })
    editor.onDidChangeCursorPosition(() => {
      pushEditorCursor()
    })
    editor.onDidScrollChange(() => {
      saveViewState(useWorkspaceFilesStore.getState().activeTab)
    })

    pushEditorCursor()

    const restoreViewStateFor = (rel: string | null) => {
      if (!rel) return
      const persisted = useWorkspaceFilesStore.getState().tabViewStates[rel]
      if (!persisted) return
      const e = editorRef.current
      if (!e) return
      try {
        if (persisted.selection) {
          e.setSelection(persisted.selection)
        }
        if (typeof persisted.scrollTop === 'number') {
          e.setScrollTop(persisted.scrollTop)
        }
        if (typeof persisted.scrollLeft === 'number') {
          e.setScrollLeft(persisted.scrollLeft)
        }
      } catch {
        /* ignore */
      }
    }

    editor.onDidChangeModel(() => {
      const model = editor.getModel()
      if (model) ensureProviders(model.getLanguageId())
      registerCurrentModel()
      const rel = useWorkspaceFilesStore.getState().activeTab
      window.setTimeout(() => restoreViewStateFor(rel), 0)
    })

    {
      const rel = useWorkspaceFilesStore.getState().activeTab
      window.setTimeout(() => restoreViewStateFor(rel), 0)
    }
  }, [addToast, handleSave, t])

  useEffect(() => {
    if (!pendingNavigation) return
    if (pendingNavigation.relPath !== activeTab) return
    let cancelled = false
    const tick = (attempts: number) => {
      if (cancelled) return
      const editor = editorRef.current
      if (!editor) {
        if (attempts > 0) setTimeout(() => tick(attempts - 1), 50)
        return
      }
      const model = editor.getModel()
      if (!model) {
        if (attempts > 0) setTimeout(() => tick(attempts - 1), 50)
        return
      }
      const lineNumber = pendingNavigation.line + 1
      const column = pendingNavigation.character + 1
      const range: MonacoNs.IRange = {
        startLineNumber: lineNumber,
        endLineNumber: lineNumber,
        startColumn: column,
        endColumn: column,
      }
      try {
        editor.revealRangeInCenter(range)
        editor.setPosition({ lineNumber, column })
        editor.focus()
      } catch {
        /* ignore */
      }
      consumeNavigation()
    }
    tick(20)
    return () => {
      cancelled = true
    }
  }, [activeTab, consumeNavigation, pendingNavigation])

  useEffect(() => {
    return () => {
      const editor = editorRef.current
      const model = editor?.getModel?.()
      if (!model) return
      const store = useWorkspaceFilesStore.getState()
      for (const [rel, registered] of Object.entries(store.monacoModels)) {
        if (registered === (model as unknown as MonacoModelHandle)) {
          store.unregisterMonacoModel(rel, registered)
        }
      }
    }
  }, [])

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
        <EditorHeader
          name={nameOf(activeTab)}
          relPath={activeTab}
          sizeBytes={buffer.sizeBytes}
          canReveal={canReveal}
          onCopyAbsolutePath={handleCopyAbsolutePath}
          onCopyRelativePath={handleCopyRelativePath}
          onReveal={handleReveal}
        />
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
        sizeBytes={buffer.sizeBytes}
        isDirty={buffer.isDirty}
        saving={buffer.saving}
        onSave={handleSave}
        saveError={buffer.saveError}
        canReveal={canReveal}
        onCopyAbsolutePath={handleCopyAbsolutePath}
        onCopyRelativePath={handleCopyRelativePath}
        onReveal={handleReveal}
        editorPrefs={editorPrefs}
        onTogglePref={togglePref}
        isMarkdown={isMarkdown}
        markdownView={markdownView}
        onChangeMarkdownView={setMarkdownView}
      />
      {showLargeFileGuard && (
        <div className="flex flex-shrink-0 flex-col gap-2 border-b border-[var(--color-warning)]/30 bg-[var(--color-warning)]/10 px-3 py-2 text-[11px] text-[var(--color-text-primary)]">
          <div className="flex items-start gap-2">
            <span className="material-symbols-outlined text-[14px] text-[var(--color-warning)]">
              warning
            </span>
            <div className="flex flex-col gap-0.5">
              <span className="font-medium">{t('files.preview.largeFileTitle')}</span>
              <span className="text-[var(--color-text-secondary)]">
                {t('files.preview.largeFileBody', {
                  size: formatBytes(buffer.sizeBytes),
                  lines: totalLines.toLocaleString(),
                })}
              </span>
            </div>
          </div>
          <div className="flex gap-1.5">
            <button
              type="button"
              onClick={() =>
                setLargeFileAck({ relPath: activeTab, mode: 'truncate' })
              }
              className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-0.5 text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
            >
              {t('files.preview.largeFileTruncate', {
                count: LARGE_FILE_TRUNCATE_LINES.toLocaleString(),
              })}
            </button>
            <button
              type="button"
              onClick={() => setLargeFileAck({ relPath: activeTab, mode: 'open' })}
              className="rounded bg-[var(--color-warning)] px-2 py-0.5 font-medium text-white hover:opacity-90"
            >
              {t('files.preview.largeFileOpen')}
            </button>
          </div>
        </div>
      )}
      {truncated && (
        <div className="flex flex-shrink-0 items-center gap-2 border-b border-[var(--color-warning)]/30 bg-[var(--color-warning)]/10 px-3 py-1.5 text-[11px] text-[var(--color-text-secondary)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-warning)]">
            content_cut
          </span>
          <span className="flex-1">
            {t('files.preview.largeFileTruncated', {
              count: LARGE_FILE_TRUNCATE_LINES.toLocaleString(),
              total: totalLines.toLocaleString(),
            })}
          </span>
        </div>
      )}
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
      <div className="relative min-h-0 flex-1" data-workspace-editor="true">
        {isMarkdown && markdownView === 'preview' && !showLargeFileGuard ? (
          <div className="markdown-preview-pane h-full overflow-auto bg-[var(--color-surface)] px-6 py-4">
            <MarkdownRenderer content={editorValue} variant="document" />
          </div>
        ) : showLargeFileGuard ? (
          <div className="flex h-full items-center justify-center px-4 text-center text-xs text-[var(--color-text-tertiary)]">
            {t('files.preview.largeFileTitle')}
          </div>
        ) : truncated ? (
          <StaticCodeViewer
            code={draftText}
            language={languageId}
            maxLines={LARGE_FILE_TRUNCATE_LINES}
            initialLines={LARGE_FILE_TRUNCATE_LINES}
          />
        ) : (
          <Editor
            path={fileUri ?? activeTab}
            theme={theme === 'dark' ? 'vs-dark' : 'vs'}
            language={languageId}
            value={editorValue}
            onMount={onMount}
            onChange={(value) => {
              if (typeof value === 'string' && !truncated) {
                updateDraft(activeTab, value)
              }
            }}
            options={{
              automaticLayout: true,
              fontSize: 13,
              lineHeight: 20,
              minimap: { enabled: editorPrefs.minimap, renderCharacters: false },
              scrollBeyondLastLine: false,
              smoothScrolling: true,
              renderWhitespace: editorPrefs.whitespace ? 'all' : 'selection',
              tabSize: 2,
              wordWrap: editorPrefs.wordWrap ? 'on' : 'off',
              wordBasedSuggestions: 'currentDocument',
              quickSuggestions: { other: true, comments: false, strings: true },
              suggestOnTriggerCharacters: true,
              fixedOverflowWidgets: true,
              bracketPairColorization: { enabled: true },
              guides: { indentation: true, bracketPairs: true, highlightActiveIndentation: true },
              parameterHints: { enabled: true, cycle: true },
              hover: { enabled: true, delay: hoverDelayMs },
              suggest: { showIcons: true, preview: true },
              inlineSuggest: { enabled: true },
              stickyScroll: { enabled: true },
              renderLineHighlight: 'all',
              folding: true,
              showFoldingControls: 'mouseover',
              readOnly: truncated,
              formatOnPaste: true,
              formatOnType: true,
              inlayHints: { enabled: inlayHintsEnabled ? 'on' : 'off' },
              lightbulb: { enabled: 'on' as MonacoNs.editor.ShowLightbulbIconMode },
            }}
          />
        )}
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

type EditorHeaderProps = {
  name: string
  relPath: string
  sizeBytes?: number
  isDirty?: boolean
  saving?: boolean
  onSave?: () => void
  saveError?: string
  canReveal?: boolean
  onCopyAbsolutePath?: () => void
  onCopyRelativePath?: () => void
  onReveal?: () => void
  editorPrefs?: EditorPrefs
  onTogglePref?: (key: keyof EditorPrefs) => void
  isMarkdown?: boolean
  markdownView?: 'source' | 'preview'
  onChangeMarkdownView?: (next: 'source' | 'preview') => void
}

function EditorHeader({
  name,
  relPath,
  sizeBytes,
  isDirty,
  saving,
  onSave,
  saveError,
  canReveal,
  onCopyAbsolutePath,
  onCopyRelativePath,
  onReveal,
  editorPrefs,
  onTogglePref,
  isMarkdown,
  markdownView,
  onChangeMarkdownView,
}: EditorHeaderProps) {
  const t = useTranslation()
  const breadcrumbs = useMemo(() => relPath.split('/'), [relPath])
  return (
    <div className="flex h-7 flex-shrink-0 items-center gap-1.5 border-b border-[var(--color-border)] bg-[var(--color-surface-container)] px-2 text-xs">
      <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
        edit_note
      </span>
      <div
        className="flex min-w-0 flex-1 items-center gap-0.5 truncate"
        title={relPath}
      >
        {breadcrumbs.length > 1 &&
          breadcrumbs.slice(0, -1).map((seg, i) => (
            <span key={`${i}-${seg}`} className="flex items-center gap-0.5">
              <span className="truncate text-[var(--color-text-tertiary)]">{seg}</span>
              <span className="material-symbols-outlined text-[12px] text-[var(--color-text-tertiary)]/60">
                chevron_right
              </span>
            </span>
          ))}
        <span className="truncate font-medium text-[var(--color-text-primary)]">{name}</span>
      </div>
      {isDirty && (
        <span
          className="inline-block size-1.5 rounded-full bg-[var(--color-accent)]"
          title={t('files.unsavedChanges')}
          aria-label={t('files.unsavedChanges')}
        />
      )}
      {typeof sizeBytes === 'number' && sizeBytes > 0 && (
        <span className="hidden text-[10px] tabular-nums text-[var(--color-text-tertiary)] sm:inline">
          {formatBytes(sizeBytes)}
        </span>
      )}
      {saveError && (
        <span
          className="truncate text-[10px] text-[var(--color-danger)]"
          title={saveError}
        >
          {saveError}
        </span>
      )}
      <div className="flex items-center gap-0.5">
        {isMarkdown && onChangeMarkdownView && (
          <div className="mr-1 flex h-5 items-center overflow-hidden rounded border border-[var(--color-border)] text-[10px]">
            <button
              type="button"
              onClick={() => onChangeMarkdownView('source')}
              className={`flex h-full items-center px-1.5 ${
                markdownView === 'source'
                  ? 'bg-[var(--color-accent)]/15 text-[var(--color-text-primary)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
              }`}
            >
              {t('files.preview.markdownSource')}
            </button>
            <button
              type="button"
              onClick={() => onChangeMarkdownView('preview')}
              className={`flex h-full items-center px-1.5 ${
                markdownView === 'preview'
                  ? 'bg-[var(--color-accent)]/15 text-[var(--color-text-primary)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
              }`}
            >
              {t('files.preview.markdownPreview')}
            </button>
          </div>
        )}
        {editorPrefs && onTogglePref && (
          <>
            <ToolbarIconButton
              icon="wrap_text"
              active={editorPrefs.wordWrap}
              label={t('files.preview.wordWrap')}
              onClick={() => onTogglePref('wordWrap')}
            />
            <ToolbarIconButton
              icon="map"
              active={editorPrefs.minimap}
              label={t('files.preview.minimap')}
              onClick={() => onTogglePref('minimap')}
            />
            <ToolbarIconButton
              icon="format_paragraph"
              active={editorPrefs.whitespace}
              label={t('files.preview.whitespace')}
              onClick={() => onTogglePref('whitespace')}
            />
          </>
        )}
        {onCopyAbsolutePath && (
          <ToolbarIconButton
            icon="content_copy"
            label={t('files.preview.copyPath')}
            onClick={onCopyAbsolutePath}
          />
        )}
        {onCopyRelativePath && (
          <ToolbarIconButton
            icon="link"
            label={t('files.preview.copyRelativePath')}
            onClick={onCopyRelativePath}
          />
        )}
        {canReveal && onReveal && (
          <ToolbarIconButton
            icon="folder_open"
            label={t('files.preview.reveal')}
            onClick={onReveal}
          />
        )}
        {onSave && (
          <button
            type="button"
            onClick={onSave}
            disabled={saving || !isDirty}
            className="ml-1 flex h-5 items-center gap-1 rounded px-2 text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40 disabled:hover:bg-transparent"
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

function ToolbarIconButton({
  icon,
  label,
  active,
  onClick,
}: {
  icon: string
  label: string
  active?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={`flex h-5 w-5 items-center justify-center rounded ${
        active
          ? 'bg-[var(--color-accent)]/15 text-[var(--color-accent)]'
          : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
      }`}
    >
      <span className="material-symbols-outlined text-[14px]">{icon}</span>
    </button>
  )
}
