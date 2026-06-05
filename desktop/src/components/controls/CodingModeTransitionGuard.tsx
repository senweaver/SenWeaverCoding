// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useShallow } from 'zustand/react/shallow'
import { ConfirmDialog } from '../shared/ConfirmDialog'
import { useTranslation, useCodingModeText } from '../../i18n'
import { useSettingsStore } from '../../stores/settingsStore'
import { useChatStore } from '../../stores/chatStore'

export function CodingModeTransitionGuard() {
  const t = useTranslation()
  const tCodingMode = useCodingModeText()
  const { pending, codingModes, resolve } = useSettingsStore(
    useShallow((s) => ({
      pending: s.pendingCodingModeTransition,
      codingModes: s.codingModes,
      resolve: s.resolveCodingModeTransition,
    })),
  )
  const sessionPending = useChatStore((s) => s.pendingSessionCodingMode)
  const resolveSession = useChatStore((s) => s.resolveSessionCodingMode)

  const targetId = pending?.target ?? ''
  const backendLabel = codingModes.find((m) => m.id === targetId)?.label
  const targetLabel = targetId
    ? tCodingMode(targetId, 'label', backendLabel ?? targetId)
    : ''

  const sessionTargetId = sessionPending?.mode ?? ''
  const sessionBackendLabel = codingModes.find((m) => m.id === sessionTargetId)?.label
  const sessionTargetLabel = sessionTargetId
    ? tCodingMode(sessionTargetId, 'label', sessionBackendLabel ?? sessionTargetId)
    : ''

  return (
    <>
      <ConfirmDialog
        open={!!pending}
        onClose={() => resolve(false)}
        onConfirm={() => resolve(true)}
        title={t('settings.agents.autoRun.confirmTransition.title')}
        body={t('settings.agents.autoRun.confirmTransition.body', { target: targetLabel })}
        confirmLabel={t('settings.agents.autoRun.confirmTransition.confirm')}
        cancelLabel={t('settings.agents.autoRun.confirmTransition.cancel')}
        confirmVariant="primary"
      />
      <ConfirmDialog
        open={!!sessionPending}
        onClose={() => resolveSession(false)}
        onConfirm={() => resolveSession(true)}
        title={t('settings.agents.autoRun.confirmTransition.title')}
        body={t('settings.agents.autoRun.confirmTransition.body', {
          target: sessionTargetLabel,
        })}
        confirmLabel={t('settings.agents.autoRun.confirmTransition.confirm')}
        cancelLabel={t('settings.agents.autoRun.confirmTransition.cancel')}
        confirmVariant="primary"
      />
    </>
  )
}
