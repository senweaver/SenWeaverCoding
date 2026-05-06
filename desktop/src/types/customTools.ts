export type CustomToolDef = {
  name: string
  description: string
  command: string
  args: string[]
  cwd: string | null
  env: Record<string, string>
  timeoutSecs: number
  schema: Record<string, unknown> | unknown
  enabled: boolean
}

export type CustomToolPatch = Partial<Omit<CustomToolDef, 'name'>> & {
  name?: string
}
