// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const EXT_LANGUAGE_MAP: Record<string, string> = {
  ts: 'typescript',
  tsx: 'tsx',
  mts: 'typescript',
  cts: 'typescript',
  js: 'javascript',
  jsx: 'jsx',
  mjs: 'javascript',
  cjs: 'javascript',
  py: 'python',
  pyi: 'python',
  rs: 'rust',
  go: 'go',
  rb: 'ruby',
  java: 'java',
  kt: 'kotlin',
  kts: 'kotlin',
  swift: 'swift',
  c: 'c',
  h: 'c',
  cc: 'cpp',
  cxx: 'cpp',
  cpp: 'cpp',
  hpp: 'cpp',
  hh: 'cpp',
  hxx: 'cpp',
  cs: 'csharp',
  php: 'php',
  lua: 'lua',
  pl: 'perl',
  scala: 'scala',
  sc: 'scala',
  dart: 'dart',
  groovy: 'groovy',
  json: 'json',
  json5: 'json5',
  jsonc: 'jsonc',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  ini: 'ini',
  env: 'dotenv',
  md: 'markdown',
  markdown: 'markdown',
  mdx: 'mdx',
  rst: 'restructuredtext',
  tex: 'latex',
  latex: 'latex',
  css: 'css',
  scss: 'scss',
  sass: 'sass',
  less: 'less',
  html: 'html',
  htm: 'html',
  xml: 'xml',
  svg: 'xml',
  vue: 'vue',
  svelte: 'svelte',
  sql: 'sql',
  graphql: 'graphql',
  gql: 'graphql',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  ps1: 'powershell',
  psm1: 'powershell',
  bat: 'bat',
  cmd: 'bat',
  fish: 'fish',
  dockerfile: 'dockerfile',
  makefile: 'makefile',
  mk: 'makefile',
  cmake: 'cmake',
  proto: 'proto',
  diff: 'diff',
  patch: 'diff',
  hcl: 'hcl',
  tf: 'hcl',
  r: 'r',
  jl: 'julia',
  ex: 'elixir',
  exs: 'elixir',
  erl: 'erlang',
  hs: 'haskell',
  clj: 'clojure',
  cljs: 'clojure',
  edn: 'clojure',
  asm: 'asm',
  s: 'asm',
  vim: 'vim',
  zig: 'zig',
  nim: 'nim',
}

const FILENAME_LANGUAGE_MAP: Record<string, string> = {
  dockerfile: 'dockerfile',
  'docker-compose.yml': 'yaml',
  'docker-compose.yaml': 'yaml',
  makefile: 'makefile',
  cmakelists: 'cmake',
  'cmakelists.txt': 'cmake',
  '.gitignore': 'gitignore',
  '.gitattributes': 'gitattributes',
  '.editorconfig': 'editorconfig',
  'cargo.toml': 'toml',
  'cargo.lock': 'toml',
}

export function inferLanguageFromPath(p: string): string {
  if (!p) return 'text'
  const norm = p.replace(/\\/g, '/').toLowerCase()
  const base = norm.split('/').filter(Boolean).pop() ?? norm
  const direct = FILENAME_LANGUAGE_MAP[base]
  if (direct) return direct
  const dot = base.lastIndexOf('.')
  if (dot <= 0) {
    const direct2 = FILENAME_LANGUAGE_MAP[base]
    if (direct2) return direct2
    return 'text'
  }
  const ext = base.slice(dot + 1)
  return EXT_LANGUAGE_MAP[ext] ?? 'text'
}

export function languageToMarkdownLang(lang: string | null | undefined): string {
  if (!lang) return ''
  switch (lang) {
    case 'plaintext':
    case 'text':
    case '':
      return ''
    default:
      return lang
  }
}
