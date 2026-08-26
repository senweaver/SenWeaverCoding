// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'
import { isCredentialGroup } from '../../api/credentials'
import { useCredentialsStore } from '../../stores/credentialsStore'
import {
  credChipLabel,
  makeCredToken,
  makeRefToken,
  parseRefSegments,
  refIconName,
  refKind,
} from './composerRefs'

export type RichComposerHandle = {
  focus: () => void
  getValue: () => string
  getCaret: () => number
  setValue: (value: string, caret?: number) => void
  clear: () => void
  insertRef: (rangeStart: number, rangeEnd: number, name: string, relPath: string) => void
  insertRefAtLastCaret: (name: string, relPath: string) => void
  replaceRange: (rangeStart: number, rangeEnd: number, text: string) => void
  insertText: (text: string) => void
}

type Props = {
  className?: string
  placeholder?: string
  disabled?: boolean
  ariaLabel?: string
  dataRole?: string
  initialValue?: string
  onChange: (value: string, caret: number) => void
  onKeyDown?: (event: React.KeyboardEvent) => void
  onPaste?: (event: React.ClipboardEvent) => void
  onCompositionStart?: () => void
  onCompositionEnd?: () => void
  onBlur?: () => void
}

const CHIP_CLASS_BASE =
  'sen-ref-chip group/chip mx-0.5 inline-flex select-none items-center gap-1 rounded-md pl-1.5 pr-1 align-middle text-[var(--color-text-secondary)]'

function chipBgClass(relPath: string): string {
  return refKind(relPath) === 'session'
    ? 'bg-[var(--color-ref-chip-session-bg)]'
    : 'bg-[var(--color-surface-container-high)]'
}

function serializedLength(node: Node): number {
  if (node.nodeType === Node.TEXT_NODE) return (node.textContent ?? '').length
  if (node.nodeType === Node.ELEMENT_NODE) {
    const el = node as HTMLElement
    if (el.dataset.refToken != null) return el.dataset.refToken.length
    if (el.dataset.credToken != null) return el.dataset.credToken.length
    if (el.tagName === 'BR') return 1
    let total = 0
    el.childNodes.forEach((child) => {
      total += serializedLength(child)
    })
    return total
  }
  return 0
}

const REF_TOKEN_PRESENCE = /@\[[^\]\n]*]\([^)\n]*\)/

function isBlankValue(value: string): boolean {
  if (REF_TOKEN_PRESENCE.test(value)) return false
  return value.replace(/[\s\u00A0]+/g, '').length === 0
}

function serializeRoot(root: HTMLElement): string {
  if (
    root.childNodes.length === 1 &&
    root.firstChild?.nodeType === Node.ELEMENT_NODE &&
    (root.firstChild as HTMLElement).tagName === 'BR'
  ) {
    return ''
  }
  let out = ''
  root.childNodes.forEach((node) => {
    out += serializeNode(node)
  })
  return out
}

function serializeNode(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? ''
  if (node.nodeType === Node.ELEMENT_NODE) {
    const el = node as HTMLElement
    if (el.dataset.refToken != null) return el.dataset.refToken
    if (el.dataset.credToken != null) return el.dataset.credToken
    if (el.tagName === 'BR') return '\n'
    let out = ''
    el.childNodes.forEach((child) => {
      out += serializeNode(child)
    })
    return out
  }
  return ''
}

function makeChipElement(name: string, relPath: string): HTMLElement {
  const span = document.createElement('span')
  span.contentEditable = 'false'
  span.dataset.refToken = makeRefToken(name, relPath)
  span.dataset.relpath = relPath
  span.dataset.refKind = refKind(relPath)
  span.className = `${CHIP_CLASS_BASE} ${chipBgClass(relPath)}`
  span.title = relPath

  const icon = document.createElement('span')
  icon.className = 'material-symbols-outlined text-[13px] leading-none'
  icon.textContent = refIconName(relPath)
  span.appendChild(icon)

  const label = document.createElement('span')
  label.className = 'text-[11px] font-medium text-[var(--color-text-primary)]'
  label.textContent = name || relPath
  span.appendChild(label)

  const remove = document.createElement('span')
  remove.dataset.refRemove = 'true'
  remove.className =
    'material-symbols-outlined ml-0.5 cursor-pointer rounded text-[13px] leading-none text-[var(--color-text-tertiary)] opacity-0 transition-opacity pointer-events-none hover:text-[var(--color-text-primary)] group-hover/chip:opacity-100 group-hover/chip:pointer-events-auto'
  remove.textContent = 'close'
  span.appendChild(remove)

  return span
}

function makeCredChipElement(name: string, field?: string): HTMLElement {
  const span = document.createElement('span')
  span.contentEditable = 'false'
  span.dataset.credToken = makeCredToken(name, field)
  span.className = `${CHIP_CLASS_BASE} bg-[var(--color-ref-chip-cred-bg)]`

  const meta = useCredentialsStore.getState().credentials.find((c) => c.name === name)
  const group = !field && meta != null && isCredentialGroup(meta)
  const fieldCount = meta?.fields?.length ?? 0
  span.title = group
    ? fieldCount > 0
      ? `${name} (${fieldCount})`
      : name
    : credChipLabel(name, field)

  const icon = document.createElement('span')
  icon.className = 'material-symbols-outlined text-[13px] leading-none'
  icon.textContent = group ? 'vpn_key' : 'key'
  span.appendChild(icon)

  const label = document.createElement('span')
  label.className = 'text-[11px] font-medium text-[var(--color-text-primary)]'
  label.textContent = credChipLabel(name, field)
  span.appendChild(label)

  const remove = document.createElement('span')
  remove.dataset.refRemove = 'true'
  remove.className =
    'material-symbols-outlined ml-0.5 cursor-pointer rounded text-[13px] leading-none text-[var(--color-text-tertiary)] opacity-0 transition-opacity pointer-events-none hover:text-[var(--color-text-primary)] group-hover/chip:opacity-100 group-hover/chip:pointer-events-auto'
  remove.textContent = 'close'
  span.appendChild(remove)

  return span
}

function renderInto(root: HTMLElement, value: string): void {
  root.textContent = ''
  for (const segment of parseRefSegments(value)) {
    if (segment.type === 'text') {
      if (segment.text) root.appendChild(document.createTextNode(segment.text))
    } else if (segment.type === 'cred') {
      root.appendChild(makeCredChipElement(segment.name, segment.field))
    } else {
      root.appendChild(makeChipElement(segment.name, segment.relPath))
    }
  }
}

function readCaret(root: HTMLElement): number {
  const selection = window.getSelection()
  if (!selection || selection.rangeCount === 0) return serializedLength(root)
  const range = selection.getRangeAt(0)
  const container = range.startContainer
  if (container !== root && !root.contains(container)) return serializedLength(root)

  if (container === root) {
    let offset = 0
    for (let i = 0; i < range.startOffset; i += 1) {
      const child = root.childNodes[i]
      if (child) offset += serializedLength(child)
    }
    return offset
  }

  let offset = 0
  let found = false
  const walk = (node: Node): void => {
    if (found) return
    if (node === container) {
      if (node.nodeType === Node.TEXT_NODE) {
        offset += range.startOffset
      } else {
        for (let i = 0; i < range.startOffset; i += 1) {
          const child = node.childNodes[i]
          if (child) offset += serializedLength(child)
        }
      }
      found = true
      return
    }
    if (node.nodeType === Node.TEXT_NODE) {
      offset += (node.textContent ?? '').length
      return
    }
    if (node.nodeType === Node.ELEMENT_NODE) {
      const el = node as HTMLElement
      if (el.dataset.refToken != null) {
        offset += el.dataset.refToken.length
        return
      }
      if (el.tagName === 'BR') {
        offset += 1
        return
      }
      el.childNodes.forEach(walk)
    }
  }
  root.childNodes.forEach(walk)
  return found ? offset : serializedLength(root)
}

function placeCaret(root: HTMLElement, target: number): void {
  const selection = window.getSelection()
  if (!selection) return
  const range = document.createRange()
  let remaining = target
  let placed = false
  for (const node of Array.from(root.childNodes)) {
    const len = serializedLength(node)
    if (remaining <= len) {
      if (node.nodeType === Node.TEXT_NODE) {
        const max = (node.textContent ?? '').length
        range.setStart(node, Math.max(0, Math.min(remaining, max)))
      } else if (remaining <= 0) {
        range.setStartBefore(node)
      } else {
        range.setStartAfter(node)
      }
      placed = true
      break
    }
    remaining -= len
  }
  if (!placed) {
    range.selectNodeContents(root)
    range.collapse(false)
  } else {
    range.collapse(true)
  }
  selection.removeAllRanges()
  selection.addRange(range)
}

export const RichComposer = forwardRef<RichComposerHandle, Props>(function RichComposer(
  {
    className,
    placeholder,
    disabled,
    ariaLabel,
    dataRole,
    initialValue,
    onChange,
    onKeyDown,
    onPaste,
    onCompositionStart,
    onCompositionEnd,
    onBlur,
  },
  ref,
) {
  const editorRef = useRef<HTMLDivElement>(null)
  const onChangeRef = useRef(onChange)
  onChangeRef.current = onChange
  const initialValueRef = useRef(initialValue ?? '')
  const lastCaretRef = useRef(-1)
  const [isEmpty, setIsEmpty] = useState(!initialValue)

  const emit = useCallback(() => {
    const root = editorRef.current
    if (!root) return
    const value = serializeRoot(root)
    const caret = readCaret(root)
    lastCaretRef.current = caret
    setIsEmpty(isBlankValue(value))
    onChangeRef.current(value, caret)
  }, [])

  const syncCaret = useCallback(() => {
    const root = editorRef.current
    if (!root) return
    lastCaretRef.current = readCaret(root)
  }, [])

  const handleMouseDown = useCallback((event: React.MouseEvent) => {
    const target = event.target as HTMLElement
    const removeBtn = target.closest('[data-ref-remove]')
    if (!removeBtn) return
    const chip = removeBtn.closest('[data-ref-token],[data-cred-token]') as HTMLElement | null
    const root = editorRef.current
    if (!chip || !root) return
    event.preventDefault()
    let offset = 0
    let sibling = root.firstChild
    while (sibling && sibling !== chip) {
      offset += serializedLength(sibling)
      sibling = sibling.nextSibling
    }
    const next = chip.nextSibling
    chip.remove()
    if (
      next &&
      next.nodeType === Node.TEXT_NODE &&
      /^[ \u00A0]/.test(next.textContent ?? '')
    ) {
      next.textContent = (next.textContent ?? '').slice(1)
    }
    let value = serializeRoot(root)
    if (isBlankValue(value)) {
      root.textContent = ''
      value = ''
      offset = 0
    }
    root.focus()
    placeCaret(root, offset)
    setIsEmpty(isBlankValue(value))
    onChangeRef.current(value, offset)
  }, [])

  const applyValue = useCallback(
    (value: string, caret?: number) => {
      const root = editorRef.current
      if (!root) return
      renderInto(root, value)
      const target = caret == null ? value.length : caret
      placeCaret(root, target)
      setIsEmpty(isBlankValue(value))
      onChangeRef.current(value, target)
    },
    [],
  )

  const insertRefAtLastCaret = useCallback(
    (name: string, relPath: string) => {
      const root = editorRef.current
      if (!root) return
      const current = serializeRoot(root)
      const at =
        lastCaretRef.current < 0
          ? current.length
          : Math.min(Math.max(lastCaretRef.current, 0), current.length)
      const leading = at > 0 && !/\s/.test(current[at - 1] ?? '') ? ' ' : ''
      const insert = `${leading}${makeRefToken(name, relPath)} `
      const next = current.slice(0, at) + insert + current.slice(at)
      applyValue(next, at + insert.length)
      lastCaretRef.current = at + insert.length
      root.focus()
    },
    [applyValue],
  )

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        editorRef.current?.focus()
      },
      getValue: () => (editorRef.current ? serializeRoot(editorRef.current) : ''),
      getCaret: () => (editorRef.current ? readCaret(editorRef.current) : 0),
      setValue: (value, caret) => applyValue(value, caret),
      clear: () => applyValue('', 0),
      insertRef: (rangeStart, rangeEnd, name, relPath) => {
        const root = editorRef.current
        if (!root) return
        const current = serializeRoot(root)
        const insert = `${makeRefToken(name, relPath)} `
        const next = current.slice(0, rangeStart) + insert + current.slice(rangeEnd)
        applyValue(next, rangeStart + insert.length)
        root.focus()
      },
      insertRefAtLastCaret,
      replaceRange: (rangeStart, rangeEnd, text) => {
        const root = editorRef.current
        if (!root) return
        const current = serializeRoot(root)
        const next = current.slice(0, rangeStart) + text + current.slice(rangeEnd)
        applyValue(next, rangeStart + text.length)
        root.focus()
      },
      insertText: (text) => {
        const root = editorRef.current
        if (!root) return
        const current = serializeRoot(root)
        const caret = readCaret(root)
        const next = current.slice(0, caret) + text + current.slice(caret)
        applyValue(next, caret + text.length)
      },
    }),
    [applyValue, insertRefAtLastCaret],
  )

  useEffect(() => {
    const root = editorRef.current
    if (!root) return
    const init = initialValueRef.current
    if (init) {
      renderInto(root, init)
      setIsEmpty(false)
    }
  }, [])

  return (
    <div className="relative w-full">
      <div
        ref={editorRef}
        role="textbox"
        aria-multiline="true"
        aria-label={ariaLabel}
        data-role={dataRole}
        contentEditable={!disabled}
        suppressContentEditableWarning
        dangerouslySetInnerHTML={{ __html: '' }}
        onInput={emit}
        onMouseDown={handleMouseDown}
        onKeyDown={onKeyDown}
        onKeyUp={syncCaret}
        onMouseUp={syncCaret}
        onBlur={() => {
          syncCaret()
          onBlur?.()
        }}
        onPaste={onPaste}
        onCompositionStart={onCompositionStart}
        onCompositionEnd={onCompositionEnd}
        className={`whitespace-pre-wrap break-words outline-none ${className ?? ''} ${
          disabled ? 'opacity-50' : ''
        }`}
      />
      {isEmpty && placeholder && (
        <div className="pointer-events-none absolute left-0 top-0 select-none py-1 text-[12px] leading-relaxed text-[var(--color-text-tertiary)]">
          {placeholder}
        </div>
      )}
    </div>
  )
})
