export type EvolutionExportFormatId =
  | 'openai_sft'
  | 'openai_dpo'
  | 'anthropic_messages'
  | 'hf_trl_dpo'
  | 'rl_sar'
  | 'agent_trajectory'

export type CloudTargetKindId =
  | 'openai_files'
  | 'huggingface_dataset'
  | 'rl_dataset_server'
  | 'tinker'
  | 'fireworks'
  | 'webhook'

export type EvolutionOverview = {
  enabled: boolean
  persistTrainingData: boolean
  nextStateJudgeEnabled: boolean
  judgeModel: string | null
  totalTurns: number
  lessonsTotal: number
  lessonsActive: number
  lessonHitsTotal: number
  exportsCount: number
  exportsBytes: number
  turnsFileSize: number
  eventsFileSize: number
  pushReceiptsCount: number
  exports: Array<{
    id: string
    format: EvolutionExportFormatId
    sampleCount: number
    sizeBytes: number
    createdAt: string
  }>
}

export type EvolutionLesson = {
  id: string
  title: string
  body: string
  tags: string[]
  codingMode: string | null
  sourceTurnIds: string[]
  hits: number
  enabled: boolean
  createdAt: string
  updatedAt: string
}

export type SignalWeights = {
  thumbs: number
  nextState: number
  tool: number
  verification: number
  cost: number
}

export type EvolutionConfigState = {
  enabled: boolean
  persistTrainingData: boolean
  nextStateJudgeEnabled: boolean
  judgeModel: string | null
  signalWeights: SignalWeights
  maxLessonsInPrompt: number
  lessonTokenBudget: number
  autoDistillOnSessionEnd: boolean
  export: {
    defaultFormat: EvolutionExportFormatId
    autoPush: boolean
    autoPushTargetId: string | null
    autoPushMinSamples: number
    autoPushMinIntervalHours: number
    redactWorkspacePaths: boolean
    redactSecrets: boolean
  }
}

export type EvolutionPersistenceStatus = {
  persistTrainingData: boolean
  turnsCount: number
  turnsFileSize: number
  eventsFileSize: number
  exportsCount: number
  exportsTotalBytes: number
  pushReceiptsCount: number
}

export type EvolutionExportRecord = {
  id: string
  format: EvolutionExportFormatId
  path: string
  sampleCount: number
  sizeBytes: number
  contentDigest: string
  digestAlgorithm: string
  timeWindowStart: string | null
  timeWindowEnd: string | null
  createdAt: string
}

export type CloudTarget = {
  id: string
  name: string
  kind: CloudTargetKindId
  endpoint: string
  headers: Record<string, string>
  secretRef: string | null
  defaultFormat: EvolutionExportFormatId
  enabled: boolean
  autoPush: boolean
  autoPushMinSamples: number
  autoPushMinIntervalHours: number
  lastPushedAt: string | null
  createdAt: string
}

export type PushReceiptView = {
  id: string
  exportId: string
  targetId: string
  status: string
  latencyMs: number | null
  responseExcerpt: string | null
  ts: string
}

export type PurgeScopeId = 'turns' | 'exports' | 'push_history' | 'events' | 'all'

export type PurgeReportView = {
  ok: boolean
  scope: string
  removedTurns: number
  removedExports: number
  removedPushHistory: number
  removedEvents: number
  freedBytes: number
}
