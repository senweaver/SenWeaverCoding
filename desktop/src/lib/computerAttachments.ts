// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ComputerAttachment } from '../stores/computerUseStore'

export type LocalAttachment = {
  id: string
  name: string
  mime: string
  dataBase64?: string
  text?: string
}

const IMAGE_MIMES = new Set(['image/png', 'image/jpeg', 'image/webp', 'image/gif'])
const TEXT_EXTENSIONS = ['.txt', '.md', '.markdown', '.json', '.csv', '.log', '.yaml', '.yml']
const MAX_IMAGE_BYTES = 10 * 1024 * 1024
const MAX_TEXT_BYTES = 512 * 1024
const MAX_TEXT_CHARS = 20_000

let nextAttachmentId = 1

function genId(): string {
  nextAttachmentId += 1
  return `att-${Date.now()}-${nextAttachmentId}`
}

export function isSupportedImage(mime: string): boolean {
  return IMAGE_MIMES.has(mime.toLowerCase())
}

function looksLikeTextFile(file: File): boolean {
  const mime = (file.type || '').toLowerCase()
  if (mime.startsWith('text/') || mime === 'application/json') return true
  const name = file.name.toLowerCase()
  return TEXT_EXTENSIONS.some((ext) => name.endsWith(ext))
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(reader.error ?? new Error('read failed'))
    reader.readAsDataURL(file)
  })
}

function readAsText(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(reader.error ?? new Error('read failed'))
    reader.readAsText(file)
  })
}

export async function fileToAttachment(file: File): Promise<LocalAttachment | null> {
  const mime = (file.type || '').toLowerCase()
  if (isSupportedImage(mime)) {
    if (file.size > MAX_IMAGE_BYTES) return null
    const dataUrl = await readAsDataUrl(file)
    const comma = dataUrl.indexOf(',')
    if (comma < 0) return null
    return {
      id: genId(),
      name: file.name || 'image',
      mime,
      dataBase64: dataUrl.slice(comma + 1),
    }
  }
  if (looksLikeTextFile(file)) {
    if (file.size > MAX_TEXT_BYTES) return null
    const text = await readAsText(file)
    const trimmed = text.length > MAX_TEXT_CHARS ? text.slice(0, MAX_TEXT_CHARS) : text
    if (!trimmed.trim()) return null
    return {
      id: genId(),
      name: file.name || 'document',
      mime: mime || 'text/plain',
      text: trimmed,
    }
  }
  return null
}

export function clipboardImageFiles(event: React.ClipboardEvent): File[] {
  const items = event.clipboardData?.items
  if (!items) return []
  const files: File[] = []
  for (const item of Array.from(items)) {
    if (item.kind === 'file' && isSupportedImage(item.type)) {
      const file = item.getAsFile()
      if (file) files.push(file)
    }
  }
  return files
}

export function toComputerAttachments(list: LocalAttachment[]): ComputerAttachment[] {
  return list.map((a) => ({
    name: a.name,
    mime: a.mime,
    ...(a.dataBase64 ? { dataBase64: a.dataBase64 } : {}),
    ...(a.text ? { text: a.text } : {}),
  }))
}
