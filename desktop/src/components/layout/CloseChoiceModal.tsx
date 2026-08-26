// SPDX-License-Identifier: MIT

import { useEffect, useState } from 'react'

import { Modal } from '../shared/Modal'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useSettingsStore } from '../../stores/settingsStore'
import { minimizeToTray, performSafeExit } from '../../lib/appClose'

type Choice = 'minimize' | 'exit'

export function CloseChoiceModal() {
  const t = useTranslation()
  const open = useUIStore((s) => s.closePromptOpen)
  const setClosePromptOpen = useUIStore((s) => s.setClosePromptOpen)
  const setCloseBehavior = useSettingsStore((s) => s.setCloseBehavior)

  const [choice, setChoice] = useState<Choice>('minimize')
  const [dontAskAgain, setDontAskAgain] = useState(false)

  useEffect(() => {
    if (!open) {
      setChoice('minimize')
      setDontAskAgain(false)
    }
  }, [open])

  const close = () => setClosePromptOpen(false)

  const confirm = () => {
    if (dontAskAgain) {
      void setCloseBehavior(choice)
    }
    if (choice === 'minimize') {
      void minimizeToTray()
      close()
    } else {
      void performSafeExit()
    }
  }

  const options: Array<{ value: Choice; label: string; hint: string; icon: string }> = [
    {
      value: 'minimize',
      label: t('close.prompt.minimize'),
      hint: t('close.prompt.minimizeHint'),
      icon: 'expand_more',
    },
    {
      value: 'exit',
      label: t('close.prompt.exit'),
      hint: t('close.prompt.exitHint'),
      icon: 'power_settings_new',
    },
  ]

  return (
    <Modal
      open={open}
      onClose={close}
      title={t('close.prompt.title')}
      width={460}
      footer={
        <>
          <button
            type="button"
            onClick={close}
            className="rounded-lg px-4 py-2 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
          >
            {t('close.prompt.cancel')}
          </button>
          <button
            type="button"
            onClick={confirm}
            className="rounded-lg bg-[var(--color-brand)] px-4 py-2 text-xs font-medium text-[var(--color-on-primary)] transition-opacity hover:opacity-90 border border-[var(--color-brand)]"
          >
            {t('close.prompt.confirm')}
          </button>
        </>
      }
    >
      <p className="mb-4 text-xs text-[var(--color-text-tertiary)]">{t('close.prompt.desc')}</p>

      <div className="flex flex-col gap-2">
        {options.map((opt) => {
          const active = choice === opt.value
          return (
            <button
              key={opt.value}
              type="button"
              onClick={() => setChoice(opt.value)}
              className={`flex items-start gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors ${
                active
                  ? 'border-[var(--color-brand)] bg-[var(--color-surface-hover)]'
                  : 'border-[var(--color-border)] hover:bg-[var(--color-surface-hover)]'
              }`}
            >
              <span
                className={`material-symbols-outlined text-[20px] ${
                  active ? 'text-[var(--color-brand)]' : 'text-[var(--color-text-secondary)]'
                }`}
              >
                {opt.icon}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block text-xs font-medium text-[var(--color-text-primary)]">
                  {opt.label}
                </span>
                <span className="mt-0.5 block text-xs text-[var(--color-text-tertiary)]">
                  {opt.hint}
                </span>
              </span>
            </button>
          )
        })}
      </div>

      <label className="mt-4 flex cursor-pointer items-center gap-2 text-xs text-[var(--color-text-secondary)]">
        <input
          type="checkbox"
          checked={dontAskAgain}
          onChange={(e) => setDontAskAgain(e.target.checked)}
          className="h-4 w-4 accent-[var(--color-brand)]"
        />
        {t('close.prompt.dontAskAgain')}
      </label>
    </Modal>
  )
}
