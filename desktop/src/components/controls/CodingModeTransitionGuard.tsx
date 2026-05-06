import { useShallow } from 'zustand/react/shallow'
import { ConfirmDialog } from '../shared/ConfirmDialog'
import { useTranslation } from '../../i18n'
import { useSettingsStore } from '../../stores/settingsStore'

export function CodingModeTransitionGuard() {
  const t = useTranslation()
  const { pending, codingModes, resolve } = useSettingsStore(
    useShallow((s) => ({
      pending: s.pendingCodingModeTransition,
      codingModes: s.codingModes,
      resolve: s.resolveCodingModeTransition,
    })),
  )

  const targetLabel =
    codingModes.find((m) => m.id === pending?.target)?.label ?? pending?.target ?? ''

  return (
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
  )
}
