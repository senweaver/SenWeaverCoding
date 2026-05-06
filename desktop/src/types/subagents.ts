export type DelegateAgentDef = {
  name: string
  provider: string
  model: string
  systemPrompt: string | null
  apiKey: string | null
  temperature: number | null
  maxDepth: number
  agentic: boolean
  allowedTools: string[]
  maxIterations: number
  timeoutSecs: number | null
  agenticTimeoutSecs: number | null
  skillsDirectory: string | null
}

export type DelegateAgentPatch = Partial<Omit<DelegateAgentDef, 'name'>> & {
  name?: string
}
