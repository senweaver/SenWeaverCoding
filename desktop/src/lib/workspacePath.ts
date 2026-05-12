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
