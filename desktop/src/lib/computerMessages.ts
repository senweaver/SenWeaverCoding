// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { translate, type TranslationKey } from '../i18n'
import { useSettingsStore } from '../stores/settingsStore'

const CODE_TO_KEY: Record<string, TranslationKey> = {
  busy: 'computerUse.msg.busy',
  model_init_failed: 'computerUse.msg.modelInitFailed',
  no_vision_model: 'computerUse.msg.noVisionModel',
  capture_failed: 'computerUse.msg.captureFailed',
  capture_failed_repeated: 'computerUse.msg.captureFailedRepeated',
  planning_failed: 'computerUse.msg.planningFailed',
  planning_failed_repeated: 'computerUse.msg.planningFailedRepeated',
  target_not_located: 'computerUse.msg.targetNotLocated',
  drag_target_not_located: 'computerUse.msg.dragTargetNotLocated',
  action_failed_repeated: 'computerUse.msg.actionFailedRepeated',
  step_limit_reached: 'computerUse.msg.stepLimitReached',
  recording_empty: 'computerUse.msg.recordingEmpty',
  replaying_steps: 'computerUse.msg.replayingSteps',
  replay_step_failed: 'computerUse.msg.replayStepFailed',
  replay_completed: 'computerUse.msg.replayCompleted',
  smart_replaying_steps: 'computerUse.msg.smartReplayingSteps',
  smart_replay_locating: 'computerUse.msg.smartReplayLocating',
  smart_replay_step_failed: 'computerUse.msg.smartReplayStepFailed',
  smart_replay_budget_exhausted: 'computerUse.msg.smartReplayBudget',
  smart_replay_grounding_failed: 'computerUse.msg.smartReplayGrounding',
  smart_replay_no_coords: 'computerUse.msg.smartReplayNoCoords',
  smart_replay_still_obscured: 'computerUse.msg.smartReplayObscured',
  smart_replay_not_found: 'computerUse.msg.smartReplayNotFound',
  smart_replay_completed: 'computerUse.msg.smartReplayCompleted',
  replay_missing_recording: 'computerUse.msg.replayMissingRecording',
  replay_load_failed: 'computerUse.msg.replayLoadFailed',
  start_missing_params: 'computerUse.msg.startMissingParams',
  skill_not_found: 'computerUse.msg.skillNotFound',
  recorder_capture_stopped: 'computerUse.msg.recorderCaptureStopped',
  recorder_own_filter: 'computerUse.msg.recorderOwnFilter',
  recorder_step_limit: 'computerUse.msg.recorderStepLimit',
  recorder_stopped_count: 'computerUse.msg.recorderStoppedCount',
  recorder_start_failed: 'computerUse.msg.recorderStartFailed',
  recorder_stop_failed: 'computerUse.msg.recorderStopFailed',
  no_saved_recording: 'computerUse.msg.noSavedRecording',
  skill_generate_failed: 'computerUse.msg.skillGenerateFailed',
  skill_annotating: 'computerUse.msg.skillAnnotating',
  skill_drafting: 'computerUse.msg.skillDrafting',
  steer_requires_model: 'computerUse.msg.steerRequiresModel',
  steer_takeover: 'computerUse.msg.steerTakeover',
  plan_draft_failed: 'computerUse.msg.planDraftFailed',
  attachment_too_large: 'computerUse.msg.attachmentTooLarge',
  attachment_unsupported: 'computerUse.msg.attachmentUnsupported',
  replay_iteration: 'computerUse.msg.replayIteration',
}

const NUMBER_IN_TEXT = /(\d+)/

export function localizeComputerMessage(
  code: string | null | undefined,
  message: string | null | undefined,
): string | null {
  if (!code) return message ?? null
  const key = CODE_TO_KEY[code]
  if (!key) return message ?? null
  const locale = useSettingsStore.getState().locale
  const match = message ? message.match(NUMBER_IN_TEXT) : null
  const count = match ? Number(match[1]) : undefined
  const localized = translate(locale, key, count !== undefined ? { count } : undefined)
  if (localized === key) return message ?? null
  return localized
}
