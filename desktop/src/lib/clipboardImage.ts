// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type PastedImage = {
  fileName: string
  dataBase64: string
}

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|bmp|svg|ico|avif|tiff?)$/i

export function isImageFileName(name: string | null | undefined): boolean {
  if (!name) return false
  return IMAGE_EXT.test(name.trim())
}

function extensionForType(type: string): string {
  const subtype = type.split('/')[1]?.split(';')[0]?.trim().toLowerCase() || 'png'
  if (subtype === 'jpeg') return 'jpg'
  if (subtype === 'svg+xml') return 'svg'
  return subtype
}

function readBlobAsDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error('failed to read image'))
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.readAsDataURL(blob)
  })
}

export function clipboardHasImage(data: DataTransfer | null): boolean {
  if (!data) return false
  for (const item of Array.from(data.items)) {
    if (item.kind === 'file' && item.type.startsWith('image/')) return true
  }
  for (const file of Array.from(data.files)) {
    if (file.type.startsWith('image/')) return true
  }
  return false
}

export async function extractClipboardImage(
  data: DataTransfer | null,
): Promise<PastedImage | null> {
  if (!data) return null
  let blob: File | null = null
  for (const item of Array.from(data.items)) {
    if (item.kind === 'file' && item.type.startsWith('image/')) {
      blob = item.getAsFile()
      if (blob) break
    }
  }
  if (!blob) {
    for (const file of Array.from(data.files)) {
      if (file.type.startsWith('image/')) {
        blob = file
        break
      }
    }
  }
  if (!blob) return null
  const dataBase64 = await readBlobAsDataUrl(blob)
  if (!dataBase64) return null
  const ext = extensionForType(blob.type || 'image/png')
  const fileName =
    blob.name && blob.name.trim().length > 0
      ? blob.name
      : `pasted-${Date.now()}.${ext}`
  return { fileName, dataBase64 }
}
