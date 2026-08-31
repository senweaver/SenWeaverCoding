// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
  dbFileSize: number
  pushReceiptsCount: number
  exports: Array<{
    id: string
    format: EvolutionExportFormatId
    sampleCount: number
    sizeBytes: number
    createdAt: string
  }>
  judgeWorker?: {
    running: boolean
    enqueuedTotal: number
    processed: number
    lastErrorAt: string | null
    lastErrorMessage: string | null
  }
  reflectionScheduler?: {
    running: boolean
    triggerMode: ReflectionTriggerModeId
    intervalMinutes: number
    lastTickAt: string | null
    nextTickAtEstimate: string | null
  }
  recycling?: {
    totalHarvested: number
    recent24hHarvested: number
    lastHarvestAt: string | null
  }
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
  distillEnabled: boolean
  autoDistillOnSessionEnd: boolean
  export: {
    redactWorkspacePaths: boolean
    redactSecrets: boolean
  }
}

export type EvolutionPersistenceStatus = {
  persistTrainingData: boolean
  turnsCount: number
  turnsJsonlLines: number
  turnsFileSize: number
  eventsFileSize: number
  dbFileSize: number
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

export type ExperienceRecyclingConfig = {
  enabled: boolean
  sampleRate: number
  minReward: number
  maxRetained: number
  maxReplayInPrompt: number
  replayTokenBudget: number
  redactWorkspacePaths: boolean
  redactSecrets: boolean
  redactUserText: boolean
  includeSuccesses: boolean
  includeFailures: boolean
  weightQuality: number
  weightRecency: number
  weightDiversity: number
}

export type RecycledExperienceOutcomeId = 'success' | 'failure' | 'neutral'

export type RecycledExperienceItem = {
  id: string
  sessionId: string
  turnId: string
  codingMode: string | null
  outcome: RecycledExperienceOutcomeId
  reward: number
  headline: string
  contextExcerpt: string
  responseExcerpt: string
  toolsSummary: string
  tags: string[]
  hits: number
  createdAt: string
}

export type RecyclingRecentResponse = {
  items: RecycledExperienceItem[]
  total: number
}

export type ReflectionTriggerModeId = 'manual' | 'auto' | 'scheduled'
export type ReflectionDepthId = 'quick' | 'deep'
export type ReflectionWritebackTargetId = 'lessons' | 'skills' | 'rules' | 'memory'

export type SelfReflectionConfig = {
  enabled: boolean
  triggerMode: ReflectionTriggerModeId
  depth: ReflectionDepthId
  reflectionModel: string | null
  reflectionProvider: string | null
  scheduleIntervalMinutes: number
  minTurnsForAuto: number
  failureThreshold: number
  writebackTargets: ReflectionWritebackTargetId[]
  maxLessonsPerRun: number
  maxTotalLessons: number
  includeUserThumbsDown: boolean
  lookbackTurns: number
}

export type AvailableModelEntry = {
  id: string
  providerId: string
  providerName: string
  isDefaultProvider: boolean
}

export type AvailableProviderEntry = {
  id: string
  name: string
  isDefault: boolean
  models: Array<{ id: string; name: string }>
}

export type AvailableModelsResponse = {
  models: AvailableModelEntry[]
  providers: AvailableProviderEntry[]
  total: number
  providersConfigured: number
  providersWithModels: number
  defaultProviderId: string | null
}

export type ReflectionRunStatusId =
  | 'queued'
  | 'running'
  | 'completed'
  | 'failed'
  | 'skipped'

export type ReflectionRunItem = {
  id: string
  sessionId: string | null
  trigger: string
  depth: string
  status: ReflectionRunStatusId
  model: string | null
  lessonsProduced: number
  turnsAnalyzed: number
  summary: string | null
  error: string | null
  startedAt: string
  completedAt: string | null
}

export type ReflectionSummary = {
  totalRuns: number
  completedRuns: number
  failedRuns: number
  lastRunAt: string | null
  lastStatus: ReflectionRunStatusId | string | null
  totalLessonsProduced: number
  avgLessonsPerRun: number | null
}

export type ReflectionRunsResponse = {
  items: ReflectionRunItem[]
  summary: ReflectionSummary
}
