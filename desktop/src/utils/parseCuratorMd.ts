

export type ParsedCurator = {
  slug: string
  template: string
  finalMdPath: string
  implBlueprintPath: string
  docxPath?: string
  body: string
  title: string
}

const BEGIN_MARKER = '===CURATOR_MARKDOWN_BEGIN==='
const END_MARKER = '===CURATOR_MARKDOWN_END==='

function extractScalar(text: string, key: string): string {
  const re = new RegExp(`^${key}:\\s*(.+)$`, 'm')
  const m = text.match(re)
  return m?.[1]?.trim() ?? ''
}

export function parseCuratorEnvelope(output: string): ParsedCurator | null {
  const begin = output.indexOf(BEGIN_MARKER)
  const end = output.indexOf(END_MARKER)
  if (begin < 0 || end <= begin) return null
  const inner = output.slice(begin + BEGIN_MARKER.length, end).replace(/^\r?\n/, '').replace(/\r?\n\s*$/, '')
  const separator = inner.indexOf('\n---\n')
  let header = inner
  let body = ''
  if (separator >= 0) {
    header = inner.slice(0, separator)
    body = inner.slice(separator + 5)
  }
  const slug = extractScalar(header, 'slug')
  const template = extractScalar(header, 'template')
  const finalMdPath = extractScalar(header, 'final_md_path')
  const implBlueprintPath = extractScalar(header, 'impl_blueprint_path')
  const docxPath = extractScalar(header, 'docx_path') || undefined
  if (!slug || !template) return null

  let title = slug
  for (const line of body.split('\n')) {
    const m = line.match(/^#\s+(.+?)\s*$/)
    if (m) {
      title = m[1] ?? slug
      break
    }
  }

  return {
    slug,
    template,
    finalMdPath,
    implBlueprintPath,
    docxPath,
    body: body.trim(),
    title,
  }
}
