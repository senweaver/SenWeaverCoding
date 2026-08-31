// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type {
  CloudTarget,
  EvolutionConfigState,
  EvolutionExportFormatId,
  EvolutionExportRecord,
  EvolutionLesson,
  EvolutionOverview,
  EvolutionPersistenceStatus,
  ExperienceRecyclingConfig,
  PurgeReportView,
  PurgeScopeId,
  PushReceiptView,
  RecyclingRecentResponse,
  ReflectionRunsResponse,
  SelfReflectionConfig,
} from '../types/evolution'

export const evolutionApi = {
  fetchOverview: () => api.get<EvolutionOverview>('/api/evolution/overview'),
  fetchConfig: () => api.get<EvolutionConfigState>('/api/evolution/config'),
  updateConfig: (patch: Partial<EvolutionConfigState>) =>
    api.put<{ ok: boolean }>('/api/evolution/config', patch),

  fetchLessons: (onlyEnabled?: boolean) =>
    api.get<{ items: EvolutionLesson[] }>(
      `/api/evolution/lessons${onlyEnabled ? '?only_enabled=true' : ''}`,
    ),
  updateLesson: (id: string, patch: Partial<EvolutionLesson>) =>
    api.put<EvolutionLesson>(`/api/evolution/lessons/${encodeURIComponent(id)}`, patch),
  deleteLesson: (id: string) => api.delete<{ ok: boolean }>(`/api/evolution/lessons/${encodeURIComponent(id)}`),

  recordThumb: (sessionId: string, score: 1 | 0 | -1, turnId?: string, comment?: string) =>
    api.post<{ ok: boolean; voteId: string; finalReward: number; auditOnly: boolean }>(
      '/api/evolution/thumbs',
      {
        session_id: sessionId,
        turn_id: turnId,
        score,
        comment,
      },
    ),

  distillTurn: (turnId: string) =>
    api.post<{ ok: boolean; queued: boolean; turnId: string }>('/api/evolution/distill', {
      turnId,
    }),

  rescoreAll: () =>
    api.post<{ ok: boolean; rescored: number; errors: number; totalSeen: number }>(
      '/api/evolution/rescore',
      {},
    ),

  fetchPersistence: () => api.get<EvolutionPersistenceStatus>('/api/evolution/persistence'),
  setPersistence: (persist: boolean) =>
    api.put<{ ok: boolean; persistTrainingData: boolean }>('/api/evolution/persistence', {
      persist_training_data: persist,
    }),
  purgePersistence: (scope: PurgeScopeId, beforeMs: number | null) =>
    api.post<PurgeReportView>('/api/evolution/persistence/purge', {
      scope,
      before_ms: beforeMs,
      confirm: 'I_UNDERSTAND',
    }),

  fetchExportFormats: () => api.get<{ items: Array<{ id: EvolutionExportFormatId; label: string }> }>(
    '/api/evolution/export/formats',
  ),
  fetchExports: () => api.get<{ items: EvolutionExportRecord[] }>('/api/evolution/exports'),
  createExport: (params: {
    format: EvolutionExportFormatId
    filter?: Record<string, unknown>
    options?: Record<string, unknown>
    preview?: boolean
  }) => api.post<EvolutionExportRecord>('/api/evolution/exports', params),
  deleteExport: (id: string) =>
    api.delete<{ ok: boolean }>(`/api/evolution/exports/${encodeURIComponent(id)}`),

  fetchCloudTargets: () => api.get<{ items: CloudTarget[] }>('/api/evolution/cloud/targets'),
  upsertCloudTarget: (target: Partial<CloudTarget> & {
    name: string
    kind: CloudTarget['kind']
    endpoint: string
    enabled: boolean
    auto_push?: boolean
    auto_push_min_samples?: number
    auto_push_min_interval_hours?: number
  }) => api.post<CloudTarget>('/api/evolution/cloud/targets', target),
  deleteCloudTarget: (id: string) =>
    api.delete<{ ok: boolean }>(`/api/evolution/cloud/targets/${encodeURIComponent(id)}`),
  push: (targetId: string, exportId: string) =>
    api.post<PushReceiptView>('/api/evolution/cloud/push', {
      target_id: targetId,
      export_id: exportId,
    }),
  fetchPushHistory: () => api.get<{ items: PushReceiptView[] }>('/api/evolution/cloud/history'),

  fetchRecyclingConfig: () =>
    api.get<ExperienceRecyclingConfig>('/api/evolution/recycling/config'),
  updateRecyclingConfig: (patch: Partial<ExperienceRecyclingConfig>) =>
    api.put<ExperienceRecyclingConfig>('/api/evolution/recycling/config', patch),
  fetchRecyclingRecent: () =>
    api.get<RecyclingRecentResponse>('/api/evolution/recycling/recent'),
  purgeRecycling: () =>
    api.post<{ ok: boolean; removed: number }>('/api/evolution/recycling/purge', {}),

  fetchReflectionConfig: () =>
    api.get<SelfReflectionConfig>('/api/evolution/reflection/config'),
  updateReflectionConfig: (patch: Partial<SelfReflectionConfig>) =>
    api.put<SelfReflectionConfig>('/api/evolution/reflection/config', patch),
  fetchReflectionRuns: () =>
    api.get<ReflectionRunsResponse>('/api/evolution/reflection/runs'),
  triggerReflection: (sessionId?: string | null) =>
    api.post<{ ok: boolean; runId: string }>(
      '/api/evolution/reflection/run',
      sessionId ? { sessionId } : {},
    ),
}
