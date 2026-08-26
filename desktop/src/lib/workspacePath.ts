// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding

export function joinWorkspaceAbsPath(root: string, rel: string): string {
  if (!rel) return root
  const usesBackslash = root.includes('\\') && !root.includes('/')
  if (root.endsWith('/') || root.endsWith('\\')) {
    return usesBackslash ? `${root}${rel.replace(/\//g, '\\')}` : `${root}${rel}`
  }
  if (usesBackslash) return `${root}\\${rel.replace(/\//g, '\\')}`
  return `${root}/${rel}`
}

export function workspaceAbsPathToUri(absPath: string): string {
  let p = absPath.replace(/\\/g, '/')
  if (!p.startsWith('/')) p = '/' + p
  return `file://${p}`
}

export function stripWinLongPathPrefix(path: string): string {
  if (!path) return path
  if (path.startsWith('\\\\?\\UNC\\')) return '\\\\' + path.slice('\\\\?\\UNC\\'.length)
  if (path.startsWith('\\\\?\\')) return path.slice('\\\\?\\'.length)
  return path
}

export function workspaceAbsToRel(
  root: string | null | undefined,
  absPath: string,
): string | null {
  const hadBackslash = absPath.includes('\\') || (!!root && root.includes('\\'))
  const file = stripWinLongPathPrefix(absPath).replace(/\\/g, '/').replace(/\/+$/, '')
  if (!file) return null
  const looksAbsolute = /^[A-Za-z]:\//.test(file) || file.startsWith('/')
  if (!root) {
    return looksAbsolute ? null : file.replace(/^\.\//, '')
  }
  const normRoot = stripWinLongPathPrefix(root).replace(/\\/g, '/').replace(/\/+$/, '')
  if (!normRoot) {
    return looksAbsolute ? null : file.replace(/^\.\//, '')
  }
  const foldCase =
    hadBackslash || /^[A-Za-z]:\//.test(normRoot) || /^[A-Za-z]:\//.test(file)
  const rootCmp = foldCase ? normRoot.toLowerCase() : normRoot
  const fileCmp = foldCase ? file.toLowerCase() : file
  if (fileCmp === rootCmp) return ''
  if (fileCmp.startsWith(`${rootCmp}/`)) {
    return file.slice(normRoot.length + 1)
  }
  if (!looksAbsolute) return file.replace(/^\.\//, '')
  return null
}

export function resolveWorkspaceFile(
  roots: Array<string | null | undefined>,
  filePath: string,
): { root: string; relPath: string } | null {
  const seen = new Set<string>()
  for (const root of roots) {
    if (!root) continue
    const key = stripWinLongPathPrefix(root)
      .replace(/\\/g, '/')
      .replace(/\/+$/, '')
      .toLowerCase()
    if (!key || seen.has(key)) continue
    seen.add(key)
    const relPath = workspaceAbsToRel(root, filePath)
    if (relPath) return { root, relPath }
  }
  return null
}
