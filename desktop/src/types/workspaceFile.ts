export type FileEntry = {
  name: string
  relPath: string
  isDir: boolean
  sizeBytes?: number
  modifiedAt?: string
}

export type FileTreeNode = FileEntry & {
  children?: FileTreeNode[]
  loaded?: boolean
}

export type FileContent = {
  content: string
  encoding: 'utf8' | 'base64'
  isBinary: boolean
  sizeBytes: number
  modifiedAt?: string
  mimeType?: string
}

export type FileTreeResponse = {
  root: string
  relPath: string
  entries: FileTreeNode[]
  truncated: boolean
}

export type FileSearchHit = FileEntry

export type FileSearchResponse = {
  results: FileSearchHit[]
  total: number
  limit: number
}

export type WriteFileResponse = {
  ok: boolean
  relPath: string
  sizeBytes?: number
  modifiedAt?: string
}
